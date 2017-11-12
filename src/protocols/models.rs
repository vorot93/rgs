extern crate std;
extern crate serde_json;
extern crate rgs_models as models;

use errors;

use std::sync::Arc;

pub type Config = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Debug)]
pub struct Packet {
    pub addr: std::net::SocketAddr,
    pub data: Vec<u8>,
}

pub struct ParseResult {
    pub servers: Vec<models::Server>,
    pub follow_up: Vec<(std::net::SocketAddr, Arc<Protocol>)>,
}

pub trait Protocol: std::fmt::Debug + Send + Sync {
    fn make_request(&self) -> Vec<u8>;
    fn parse_response(&self, p: &Packet) -> errors::Result<ParseResult>;
}

pub type TProtocol = Arc<Protocol>;
pub type ProtocolConfig = std::collections::HashMap<String, TProtocol>;
