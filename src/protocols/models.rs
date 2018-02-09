extern crate futures_await as futures;
extern crate rgs_models as models;
extern crate serde_json;
extern crate std;

use errors::*;
use futures::prelude::*;
use serde_json::Value;
use std::sync::Arc;

pub type Config = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Debug)]
pub struct Packet {
    pub addr: std::net::SocketAddr,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringAddr {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    A(std::net::SocketAddr),
    S(StringAddr),
}

#[derive(Clone, Debug)]
pub struct UserQuery {
    pub protocol: TProtocol,
    pub host: Host,
}

impl PartialEq for UserQuery {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host && Arc::ptr_eq(&self.protocol, &other.protocol)
    }
}

#[derive(Clone, Debug)]
pub struct Query {
    pub protocol: TProtocol,
    pub host: Host,
    pub state: Option<Value>,
}

impl From<UserQuery> for Query {
    fn from(v: UserQuery) -> Self {
        Self {
            protocol: v.protocol,
            host: v.host,
            state: None,
        }
    }
}

impl From<Query> for UserQuery {
    fn from(v: Query) -> Self {
        Self {
            protocol: v.protocol,
            host: v.host,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerResponse {
    pub host: Host,
    pub protocol: TProtocol,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum FollowUpQueryProtocol {
    This,
    Child(Arc<Protocol>),
}

#[derive(Clone, Debug)]
pub struct FollowUpQuery {
    pub host: Host,
    pub state: Option<Value>,
    pub protocol: FollowUpQueryProtocol,
}

impl From<(FollowUpQuery, TProtocol)> for Query {
    fn from(v: (FollowUpQuery, TProtocol)) -> Query {
        Query {
            host: v.0.host,
            protocol: match v.0.protocol {
                FollowUpQueryProtocol::This => v.1,
                FollowUpQueryProtocol::Child(p) => p,
            },
            state: v.0.state,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ParseResult {
    FollowUp(FollowUpQuery),
    Output(models::Server),
}

/// Protocol defines a common way to communicate with queried servers of a single type.
pub trait Protocol: std::fmt::Debug + Send + Sync {
    /// Creates a request packet. Can accept an optional state if there is any.
    fn make_request(&self, state: Option<Value>) -> Vec<u8>;
    /// Create a stream of parsed values out of incoming response.
    fn parse_response(&self, p: Packet) -> Box<Stream<Item = ParseResult, Error = Error>>;
}

pub type TProtocol = Arc<Protocol>;
pub type ProtocolConfig = std::collections::HashMap<String, TProtocol>;
