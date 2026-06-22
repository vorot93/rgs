//! End-to-end tests of the query engine against local fake game servers.

use futures::StreamExt;
use qgs::{Client, model::Host};
use std::{
    net::{SocketAddr, SocketAddrV4},
    time::Duration,
};
use tokio::net::UdpSocket;

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

/// Build a Quake III master `getserversResponse` datagram. `eot` marks the
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

fn v4(addr: SocketAddr) -> SocketAddrV4 {
    match addr {
        SocketAddr::V4(v4) => v4,
        SocketAddr::V6(_) => unreachable!("loopback bind is IPv4"),
    }
}

#[tokio::test]
async fn query_server_parses_status_response() {
    let server_addr = spawn_echo_server(status_response("Test Server", "q3dm6")).await;

    let client = Client::builder().build();
    let server = tokio::time::timeout(
        Duration::from_secs(10),
        client.query_server(None, Host::Addr(server_addr)),
    )
    .await
    .expect("query did not finish")
    .expect("query errored")
    .expect("no server returned");

    assert_eq!(server.addr, server_addr);
    assert_eq!(server.name.as_deref(), Some("Test Server"));
    assert_eq!(server.map.as_deref(), Some("q3dm6"));
    assert_eq!(server.max_clients, Some(8));
    assert!(
        server.ping.is_some(),
        "RTT must be measured from the UDP round-trip"
    );
}

/// Regression guard for the macOS IPv4-from-IPv6 `EINVAL` crash (commit 18eeaa1):
/// the default IPv4 socket must reach an IPv4 server.
#[tokio::test]
async fn reaches_ipv4_server() {
    let server_addr = spawn_echo_server(status_response("IPv4 Server", "q3dm6")).await;
    assert!(server_addr.is_ipv4());

    let client = Client::builder().build();
    let server = tokio::time::timeout(
        Duration::from_secs(10),
        client.query_server(None, Host::Addr(server_addr)),
    )
    .await
    .expect("query did not finish")
    .expect("query errored")
    .expect("no server returned");

    assert_eq!(server.name.as_deref(), Some("IPv4 Server"));
}

#[tokio::test]
async fn master_fans_out_to_per_server_queries() {
    let a = spawn_echo_server(status_response("Server A", "q3dm1")).await;
    let b = spawn_echo_server(status_response("Server B", "q3dm2")).await;
    let master_addr = spawn_echo_server(getservers_response(&[v4(a), v4(b)], true)).await;

    let client = Client::builder().build();
    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client
            .query_master(None, Host::Addr(master_addr), true)
            .collect::<Vec<_>>(),
    )
    .await
    .expect("stream did not terminate");

    let mut names: Vec<String> = entries
        .into_iter()
        .map(|r| r.unwrap().name.unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec!["Server A".to_string(), "Server B".to_string()]);
}

#[tokio::test]
async fn master_response_split_across_datagrams_is_fully_read() {
    let a = spawn_echo_server(status_response("Server A", "q3dm1")).await;
    let b = spawn_echo_server(status_response("Server B", "q3dm2")).await;
    let c = spawn_echo_server(status_response("Server C", "q3dm3")).await;
    let d = spawn_echo_server(status_response("Server D", "q3dm4")).await;

    // First datagram (A, B) is NOT terminated; second (C, D) carries `\EOT`.
    let datagram1 = getservers_response(&[v4(a), v4(b)], false);
    let datagram2 = getservers_response(&[v4(c), v4(d)], true);

    let master = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let master_addr = master.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((_, peer)) = master.recv_from(&mut buf).await {
            let _ = master.send_to(&datagram1, peer).await;
            let _ = master.send_to(&datagram2, peer).await;
        }
    });

    let client = Client::builder().build();
    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client
            .query_master(None, Host::Addr(master_addr), true)
            .collect::<Vec<_>>(),
    )
    .await
    .expect("stream did not terminate");

    let mut names: Vec<String> = entries
        .into_iter()
        .map(|r| r.unwrap().name.unwrap())
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

#[tokio::test]
async fn down_server_yields_none() {
    // Claim a port, then drop the socket so nothing listens there.
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let client = Client::builder()
        .timeout(Duration::from_millis(200))
        .build();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        client.query_server(None, Host::Addr(dead_addr)),
    )
    .await
    .expect("query did not finish")
    .expect("query errored");

    assert!(result.is_none(), "a down server must not yield a server");
}

#[tokio::test]
async fn master_without_follow_up_yields_bare_addresses() {
    let a: SocketAddrV4 = "10.0.0.1:27960".parse().unwrap();
    let b: SocketAddrV4 = "10.0.0.2:27961".parse().unwrap();
    let master_addr = spawn_echo_server(getservers_response(&[a, b], true)).await;

    let client = Client::builder().build();
    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client
            .query_master(None, Host::Addr(master_addr), false)
            .collect::<Vec<_>>(),
    )
    .await
    .expect("stream did not terminate");

    let mut addrs: Vec<SocketAddr> = entries.iter().map(|r| r.as_ref().unwrap().addr).collect();
    addrs.sort();
    assert_eq!(addrs, vec![SocketAddr::V4(a), SocketAddr::V4(b)]);

    // No getstatus is sent, so only the address is populated.
    for r in &entries {
        let s = r.as_ref().unwrap();
        assert!(s.name.is_none());
        assert!(s.players.is_none());
        assert!(s.ping.is_none());
    }
}

#[tokio::test]
async fn master_follow_up_surfaces_per_server_parse_error_and_continues() {
    // One healthy server, and one that replies with a wrong-type datagram
    // (a getserversResponse where a statusResponse is expected). The wrong-type
    // reply must surface as an Err item without aborting the run, so the healthy
    // server still yields its Ok.
    let good = spawn_echo_server(status_response("Healthy", "q3dm6")).await;
    let bad = spawn_echo_server(getservers_response(&[], true)).await;

    let master_addr = spawn_echo_server(getservers_response(&[v4(good), v4(bad)], true)).await;

    let client = Client::builder().build();
    let entries = tokio::time::timeout(
        Duration::from_secs(10),
        client
            .query_master(None, Host::Addr(master_addr), true)
            .collect::<Vec<_>>(),
    )
    .await
    .expect("stream did not terminate");

    let oks: Vec<_> = entries.iter().filter(|r| r.is_ok()).collect();
    let errs: Vec<_> = entries.iter().filter(|r| r.is_err()).collect();
    assert_eq!(oks.len(), 1, "the healthy server must yield exactly one Ok");
    assert_eq!(oks[0].as_ref().unwrap().name.as_deref(), Some("Healthy"));
    assert_eq!(
        errs.len(),
        1,
        "the wrong-type reply must yield exactly one Err"
    );
}

/// RTT must reflect the network round-trip, not the consumer's pace. A consumer
/// that stalls after the first result must not inflate later servers' pings:
/// each response is timestamped the instant it arrives, regardless of when the
/// consumer gets around to reading it. (Regression: the old single loop measured
/// RTT only when it next polled the socket, so a slow consumer inflated every
/// subsequent ping by its stall.)
#[tokio::test]
async fn ping_is_not_inflated_by_a_slow_consumer() {
    let a = spawn_echo_server(status_response("Server A", "q3dm1")).await;
    let b = spawn_echo_server(status_response("Server B", "q3dm2")).await;
    let master_addr = spawn_echo_server(getservers_response(&[v4(a), v4(b)], true)).await;

    let client = Client::builder().build();
    let mut stream = std::pin::pin!(client.query_master(None, Host::Addr(master_addr), true));

    // Take the first result, then stall the consumer far longer than any real
    // loopback RTT before taking the second.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("timed out on first result")
        .expect("stream ended early")
        .expect("first result errored");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let second = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("timed out on second result")
        .expect("stream ended early")
        .expect("second result errored");

    for server in [first, second] {
        let ping = server.ping.expect("ping must be measured");
        assert!(
            ping < Duration::from_millis(100),
            "ping {ping:?} for {:?} was inflated by the 500ms consumer stall",
            server.name,
        );
    }
}
