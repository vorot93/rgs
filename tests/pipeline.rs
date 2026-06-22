//! End-to-end tests of the query engine against local fake game servers.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};

use futures::{
    StreamExt,
    future::{BoxFuture, ready},
};
use rgs::{
    Client,
    dns::Resolver,
    model::{Host, Query},
    ping::Pinger,
    protocol::make_default_protocols,
};
use tokio::net::UdpSocket;

/// Resolver backed by a static hostname -> address table.
struct MockResolver {
    table: HashMap<String, SocketAddr>,
}

impl Resolver for MockResolver {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
        let result = match host {
            Host::Addr(addr) => Ok(addr),
            Host::Named { host, .. } => self
                .table
                .get(&host)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("unknown host {host}")),
        };
        Box::pin(ready(result))
    }
}

/// Pinger that reports a fixed round-trip time without sending ICMP.
struct MockPinger(Duration);

impl Pinger for MockPinger {
    fn ping(&self, _: IpAddr) -> BoxFuture<'static, anyhow::Result<Option<Duration>>> {
        Box::pin(ready(Ok(Some(self.0))))
    }
}

/// Build a Quake III `statusResponse` datagram.
fn status_response(name: &str, map: &str) -> Vec<u8> {
    let info = [
        ("sv_hostname", name),
        ("mapname", map),
        ("sv_maxclients", "8"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let mut out = Vec::new();
    q3a::Packet::StatusResponse(q3a::StatusResponseData {
        info,
        players: vec![],
    })
    .write_bytes(&mut out)
    .unwrap();
    out
}

/// Build a Quake III master `getserversResponse` datagram. `eot` marks it as the
/// terminating packet (q3a's writer emits the `\EOT` terminator when set).
fn getservers_response(addrs: &[SocketAddrV4], eot: bool) -> Vec<u8> {
    let mut out = Vec::new();
    q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
        data: addrs.iter().copied().collect(),
        eot,
    })
    .write_bytes(&mut out)
    .unwrap();
    out
}

/// Spawn a fake server on loopback that replies to every datagram with `reply`.
/// Returns its bound address.
async fn spawn_echo_server(reply: Vec<u8>) -> SocketAddr {
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((_, peer)) = server.recv_from(&mut buf).await {
            let _ = server.send_to(&reply, peer).await;
        }
    });
    addr
}

#[tokio::test]
async fn query_resolves_and_parses_q3s_response() {
    let server_addr = spawn_echo_server(status_response("Test Server", "q3dm6")).await;

    let ping = Duration::from_millis(123);
    let client = Client::builder()
        .resolver(Arc::new(MockResolver {
            table: HashMap::from([("fakehost".to_string(), server_addr)]),
        }))
        .pinger(Arc::new(MockPinger(ping)) as Arc<dyn Pinger>)
        .build();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3s"].clone(),
        host: Host::Named {
            host: "fakehost".to_string(),
            port: server_addr.port(),
        },
    }];

    // The stream drains on its own once the single query finishes; guard against
    // a hang with an outer timeout.
    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client.query(queries).collect::<Vec<_>>(),
    )
    .await
    .expect("query stream did not terminate");

    assert_eq!(entries.len(), 1);
    let entry = entries.into_iter().next().unwrap().unwrap();
    assert_eq!(entry.server.addr, server_addr);
    assert_eq!(entry.server.name.as_deref(), Some("Test Server"));
    assert_eq!(entry.server.map.as_deref(), Some("q3dm6"));
    assert_eq!(entry.server.max_clients, Some(8));
    // Ping comes from the mock pinger, proving that stage ran.
    assert_eq!(entry.server.ping, Some(ping));
}

/// Regression guard for the old macOS IPv4-from-IPv6 `EINVAL` crash: the engine
/// now binds each query socket to the target's own address family, so an IPv4
/// server is always reachable.
#[tokio::test]
async fn reaches_ipv4_server() {
    let server_addr = spawn_echo_server(status_response("IPv4 Server", "q3dm6")).await;
    assert!(server_addr.is_ipv4());

    let client = Client::builder()
        .resolver(Arc::new(MockResolver {
            table: HashMap::new(),
        }))
        .build();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3s"].clone(),
        host: Host::Addr(server_addr),
    }];

    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client.query(queries).collect::<Vec<_>>(),
    )
    .await
    .expect("query stream did not terminate");

    assert_eq!(entries.len(), 1);
    let entry = entries.into_iter().next().unwrap().unwrap();
    assert_eq!(entry.server.name.as_deref(), Some("IPv4 Server"));
}

#[tokio::test]
async fn master_fans_out_to_per_server_queries() {
    // Two fake q3s game servers.
    let a = spawn_echo_server(status_response("Server A", "q3dm1")).await;
    let b = spawn_echo_server(status_response("Server B", "q3dm2")).await;

    let v4s: Vec<SocketAddrV4> = [a, b]
        .iter()
        .map(|addr| match addr {
            SocketAddr::V4(v4) => *v4,
            SocketAddr::V6(_) => unreachable!("loopback bind is IPv4"),
        })
        .collect();

    // Fake master that hands out those two addresses in one terminating datagram.
    let master_addr = spawn_echo_server(getservers_response(&v4s, true)).await;

    let client = Client::builder()
        .resolver(Arc::new(MockResolver {
            table: HashMap::new(),
        }))
        .build();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3m"].clone(),
        host: Host::Addr(master_addr),
    }];

    // The outer timeout also proves the stream terminates once work drains.
    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client.query(queries).collect::<Vec<_>>(),
    )
    .await
    .expect("query stream did not terminate");

    let servers: Vec<_> = entries.into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(servers.len(), 2);

    let mut names: Vec<String> = servers
        .iter()
        .map(|s| s.server.name.clone().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec!["Server A".to_string(), "Server B".to_string()]);
}

#[tokio::test]
async fn down_server_yields_no_entry_and_stream_terminates() {
    // Claim a port, then drop the socket so nothing listens there.
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let client = Client::builder()
        .resolver(Arc::new(MockResolver {
            table: HashMap::new(),
        }))
        .timeout(Duration::from_millis(200))
        .build();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3s"].clone(),
        host: Host::Addr(dead_addr),
    }];

    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client.query(queries).collect::<Vec<_>>(),
    )
    .await
    .expect("query stream did not terminate");

    // A non-responding server is skipped (whether via recv timeout or an ICMP
    // port-unreachable recv error): it must never produce a parsed server entry,
    // and the stream must still drain to completion.
    let server_entries = entries.iter().filter(|r| r.is_ok()).count();
    assert_eq!(
        server_entries, 0,
        "a down server must not yield a server entry"
    );
}

#[tokio::test]
async fn resolver_failure_is_yielded_as_error() {
    // Empty resolver table => an unknown named host resolves to an error.
    let client = Client::builder()
        .resolver(Arc::new(MockResolver {
            table: HashMap::new(),
        }))
        .build();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3s"].clone(),
        host: Host::Named {
            host: "no-such-host".to_string(),
            port: 27960,
        },
    }];

    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client.query(queries).collect::<Vec<_>>(),
    )
    .await
    .expect("query stream did not terminate");

    // A DNS/resolution failure is a hard failure: surfaced as exactly one Err item.
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].is_err(),
        "resolver failure must be yielded as Err"
    );
}

#[tokio::test]
async fn master_response_split_across_datagrams_is_fully_read() {
    // Four fake q3s game servers, handed out two-per-datagram by the master.
    let a = spawn_echo_server(status_response("Server A", "q3dm1")).await;
    let b = spawn_echo_server(status_response("Server B", "q3dm2")).await;
    let c = spawn_echo_server(status_response("Server C", "q3dm3")).await;
    let d = spawn_echo_server(status_response("Server D", "q3dm4")).await;

    let v4 = |addr: SocketAddr| match addr {
        SocketAddr::V4(v4) => v4,
        SocketAddr::V6(_) => unreachable!("loopback bind is IPv4"),
    };

    // First datagram carries A, B and is NOT terminated (eot = false); the second
    // carries C, D and ends with `\EOT`. The engine must read both before stopping.
    let datagram1 = getservers_response(&[v4(a), v4(b)], false);
    let datagram2 = getservers_response(&[v4(c), v4(d)], true);

    // Fake master: reply to each query with the two datagrams, in order.
    let master = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let master_addr = master.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((_, peer)) = master.recv_from(&mut buf).await {
            let _ = master.send_to(&datagram1, peer).await;
            let _ = master.send_to(&datagram2, peer).await;
        }
    });

    let client = Client::builder()
        .resolver(Arc::new(MockResolver {
            table: HashMap::new(),
        }))
        .build();

    let protocols = make_default_protocols();
    let queries = vec![Query {
        protocol: protocols["q3m"].clone(),
        host: Host::Addr(master_addr),
    }];

    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client.query(queries).collect::<Vec<_>>(),
    )
    .await
    .expect("query stream did not terminate");

    // Every server from both datagrams must be queried, proving the engine kept
    // reading past the un-terminated first datagram.
    let mut names: Vec<String> = entries
        .into_iter()
        .map(|r| r.unwrap().server.name.unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "Server A".to_string(),
            "Server B".to_string(),
            "Server C".to_string(),
            "Server D".to_string(),
        ]
    );
}
