use errors::Error;
use protocols::models::{Packet, ParseResult, Protocol, Server};

use futures;
use futures::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;

const BODY_SEPARATOR: u8 = 0xa;
const RULES_SEPARATOR: u8 = 0x5c;

fn parse_rulestring(data: String) -> Result<HashMap<String, String>, Error> {
    let mut out = HashMap::<String, String>::default();
    let mut split_iter = data.split('\\');

    if let Some(v) = split_iter.next() {
        if v.len() > 0 {
            return Err(Error::DataParseError {
                reason: "First item in split should be empty".to_string(),
            });
        }

        loop {
            match split_iter.next() {
                None => {
                    break;
                }
                Some(k) => {
                    let v = split_iter.next().ok_or(Error::DataParseError {
                        reason: "Early EOL while parsing rule string".into(),
                    })?;

                    out.insert(k.into(), v.into());
                }
            }
        }
    }

    Ok(out)
}

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

fn parse_data(
    response_prelude: Vec<u8>,
    mut buf: Vec<u8>,
    addr: SocketAddr,
) -> Result<Server, Error> {
    if buf.len() < response_prelude.len() {
        return Err(Error::DataParseError {
            reason: "Packet is shorter than response prelude.".into(),
        });
    }

    if buf.drain(0..response_prelude.len()).collect::<Vec<u8>>() != response_prelude {
        return Err(Error::DataParseError {
            reason: "Packet does not match response prelude".into(),
        });
    }

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
    fn make_request(&self, state: Option<Value>) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[255, 255, 255, 255]);
        v.extend_from_slice("getstatus RGS".as_bytes());

        v
    }

    fn parse_response(&self, p: Packet) -> Box<Stream<Item = ParseResult, Error = Error>> {
        Box::new(futures::stream::iter_result(vec![
            parse_data(self.response_prelude.clone(), p.data, p.addr).map(ParseResult::Output),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rulestring() {
        let fixture =
            "\\voip\\opus\\g_needpass\\0\\pure\\1\\gametype\\0\\sv_maxclients\\8".to_string();
        let mut expectation = HashMap::<String, String>::default();
        for &(k, v) in [
            ("voip", "opus"),
            ("g_needpass", "0"),
            ("pure", "1"),
            ("gametype", "0"),
            ("sv_maxclients", "8"),
        ].iter()
        {
            expectation.insert(k.to_string(), v.to_string());
        }

        let result = parse_rulestring(fixture).unwrap();

        assert_eq!(expectation, result);
    }

    #[test]
    fn test_parse_server() {
        let addr = "77.93.223.201:27960".parse().unwrap();
        let fixture = Packet {
            addr,
            data: include_bytes!("test_payload/q3s_response.raw").to_vec(),
        };
    }
}
