use errors::Error;
use models::{Packet, ParseResult, Protocol, ProtocolResultStream, Server};

use failure;
use futures;
use futures::prelude::*;
use q3a;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Rule {
    Secure,
    MaxClients,
    GameType,
    Mod,
    Map,
    NeedPass,
    ServerName,
}

fn parse_q3a_server(pkt: q3a::Packet, rules: HashMap<Rule, String>) -> Result<Server, failure::Error> {

}

fn parse_data(
    response_prelude: Vec<u8>,
    buf: Vec<u8>,
    addr: SocketAddr,
) -> Result<Server, Error> {
    let pkt = q3a::Packet::from_bytes(buf.as_slice().into());

    let mut server = Server::new(addr);

    Ok(server)
}

/// Quake III Arena server protocol implementation
#[derive(Debug)]
pub struct Q3SProtocol {
    pub protocol_ver: u8,
    pub default_request_port: u16,
    pub response_prelude: Vec<u8>,
    pub rule_names: HashMap<Rule, String>,
}

impl Default for Q3SProtocol {
    fn default() -> Self {
        Self {
            protocol_ver: 68,
            default_request_port: 27950,
            response_prelude: {
                let mut v = Vec::new();
                v.extend_from_slice(&[255, 255, 255, 255]);
                v.extend_from_slice("statusResponse RGS".as_bytes());
                v
            },
            rule_names: hashmap! {
                Rule::Secure => "sv_punkbuster".into(),
                Rule::MaxClients => "sv_maxclients".into(),
                Rule::Mod => "game".into(),
                Rule::GameType => "g_gametype".into(),
                Rule::Map => "".into(),
                Rule::NeedPass => "g_needpass".into(),
                Rule::ServerName => "sv_hostname".into(),
            },
        }
    }
}

impl Protocol for Q3SProtocol {
    fn make_request(&self, _state: Option<Value>) -> Vec<u8> {
        q3a::Packet::GetStatus(q3a::GetStatusData { challenge: "RGS".into() }).to_bytes()
    }

    fn parse_response(&self, p: Packet) -> ProtocolResultStream {
        Box::new(futures::stream::iter_result(vec![
            parse_data(self.response_prelude.clone(), p.data, p.addr).map(ParseResult::Output),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_server() {
        let addr = "77.93.223.201:27960".parse().unwrap();
        let fixture = Packet {
            addr,
            data: include_bytes!("test_payload/q3s_response.raw").to_vec(),
        };
    }
}
