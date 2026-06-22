pub mod q3m;
pub mod q3s;

use crate::model::{Query, Server};
use std::{collections::HashMap, fmt::Debug, net::SocketAddr, sync::Arc};

/// One result of parsing a server response.
#[derive(Debug)]
pub enum Outcome {
    /// A fully parsed server.
    Server(Server),
    /// A follow-up query to run (e.g. a server address handed out by a master).
    FollowUp(Query),
}

/// The parsed result of a single response datagram.
#[derive(Debug)]
pub struct Parsed {
    pub outcomes: Vec<Outcome>,
    /// Whether the engine should keep reading datagrams on this socket.
    pub expect_more: bool,
}

/// A way to talk to one kind of game server.
pub trait Protocol: Debug + Send + Sync + 'static {
    /// The request datagram to send.
    fn make_request(&self) -> Vec<u8>;
    /// Parse one response datagram received from `addr`.
    fn parse_response(&self, addr: SocketAddr, data: &[u8]) -> anyhow::Result<Parsed>;
}

/// The protocols enabled by default, keyed by short name. The `q3m` master is
/// wired to the shared `q3s` protocol, so its follow-ups query servers directly.
pub fn make_default_protocols() -> HashMap<String, Arc<dyn Protocol>> {
    let q3s: Arc<dyn Protocol> = Arc::new(q3s::Q3s::default());
    let q3m: Arc<dyn Protocol> = Arc::new(q3m::Q3m {
        q3s_protocol: Some(q3s.clone()),
        ..Default::default()
    });

    let mut out = HashMap::new();
    out.insert("q3s".to_string(), q3s);
    out.insert("q3m".to_string(), q3m);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_protocols_contains_q3m_and_q3s() {
        let protocols = make_default_protocols();
        assert!(protocols.contains_key("q3m"));
        assert!(protocols.contains_key("q3s"));
    }

    #[test]
    fn q3m_follow_ups_target_the_shared_q3s_protocol() {
        let protocols = make_default_protocols();
        let q3s = protocols["q3s"].clone();
        let q3m = &protocols["q3m"];

        // A master response with one server address, terminated by `\EOT`.
        let mut data = Vec::new();
        q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
            data: ["10.0.0.1:27960".parse().unwrap()].into_iter().collect(),
        })
        .write_bytes(&mut data)
        .unwrap();
        data.extend_from_slice(b"\\EOT");

        let parsed = q3m
            .parse_response("5.6.7.8:27950".parse().unwrap(), &data)
            .unwrap();
        assert_eq!(parsed.outcomes.len(), 1);

        match parsed.outcomes.into_iter().next().unwrap() {
            // Pointer identity proves the master is wired to its child q3s.
            Outcome::FollowUp(f) => assert!(Arc::ptr_eq(&f.protocol, &q3s)),
            other => panic!("expected FollowUp, got {other:?}"),
        }
    }
}
