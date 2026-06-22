//! End-to-end test of the `UdpQuery` pipeline against a local fake game server.
//!
//! Exercises the full path — queued query -> (mock) DNS -> real loopback UDP ->
//! fake server -> response parsing -> (mock) ping -> emitted `ServerEntry` —
//! without touching the network or real game servers.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use futures::future::{BoxFuture, ready};
use rgs::{
    UdpQueryBuilder,
    dns::Resolver,
    models::{Host, StringAddr, UserQuery},
    ping::Pinger,
    protocols::make_default_protocols,
};
use tokio::net::UdpSocket;
use tokio_stream::StreamExt;

/// Resolver backed by a static hostname -> address table.
struct MockResolver {
    table: HashMap<String, SocketAddr>,
}

impl Resolver for MockResolver {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
        let result = match host {
            Host::A(addr) => Ok(addr),
            Host::S(s) => self
                .table
                .get(&s.host)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("unknown host {}", s.host)),
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

/// Build a Quake III `statusResponse` packet for the fake server to reply with.
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

#[tokio::test]
async fn udp_query_resolves_queries_and_parses_responses() {
    // Fake game server: reply to every datagram with a canned status response.
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((_, peer)) = server.recv_from(&mut buf).await {
            let _ = server
                .send_to(&status_response("Test Server", "q3dm6"), peer)
                .await;
        }
    });

    let ping = Duration::from_millis(123);
    let resolver = Arc::new(MockResolver {
        table: HashMap::from([("fakehost".to_string(), server_addr)]),
    });

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut query = UdpQueryBuilder::default()
        .with_dns_resolver(resolver)
        .with_pinger(Arc::new(MockPinger(ping)) as Arc<dyn Pinger>)
        .build(socket);

    let protocols = make_default_protocols();
    assert!(query.queue(UserQuery {
        protocol: protocols["q3s"].clone(),
        host: Host::S(StringAddr {
            host: "fakehost".to_string(),
            port: server_addr.port(),
        }),
    }));

    // A direct q3s query yields the parsed server as the first item; the stream
    // never ends on its own, so guard the poll with a timeout.
    let entry = tokio::time::timeout(Duration::from_secs(5), query.next())
        .await
        .expect("timed out waiting for a server entry")
        .expect("stream ended without yielding an entry")
        .expect("query returned an error");
    assert_eq!(entry.data.addr, server_addr);
    assert_eq!(entry.data.name.as_deref(), Some("Test Server"));
    assert_eq!(entry.data.map.as_deref(), Some("q3dm6"));
    assert_eq!(entry.data.max_clients, Some(8));
    // Ping comes from the mock pinger, proving that stage ran.
    assert_eq!(entry.data.ping, Some(ping));
}
