use crate::model::{Query, Server};
use std::{fmt::Debug, net::SocketAddr};

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
