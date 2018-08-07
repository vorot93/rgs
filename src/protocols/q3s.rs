use errors::{Error, Result};
use models::{Packet, ParseResult, Protocol, ProtocolResultStream, Server};

use futures;
use futures::prelude::*;
use q3a;
use serde_json::Value;
use std::collections::HashMap;

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

fn parse_q3a_server(
    srv: &mut Server,
    pkt: q3a::InfoResponseData,
    rule_mapping: HashMap<Rule, String>,
) -> Result<()> {
    use self::Rule::*;

    let mut rules = pkt.info;

    if let Some(rule) = rule_mapping.get(&ServerName) {
        srv.name = rules.remove(rule);
    }

    if let Some(rule) = rule_mapping.get(&Secure) {
        srv.secure = rules.remove(rule).map(|v| v == "1");
    }

    srv.rules = rules
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    Ok(())
}

/// Quake III Arena server protocol implementation
#[derive(Debug)]
pub struct ProtocolImpl {
    pub version: u8,
    pub rule_names: HashMap<Rule, String>,
}

impl Default for ProtocolImpl {
    fn default() -> Self {
        Self {
            version: 68,
            rule_names: hashmap! {
                Rule::Secure => "sv_punkbuster".into(),
                Rule::MaxClients => "sv_maxclients".into(),
                Rule::Mod => "game".into(),
                Rule::GameType => "g_gametype".into(),
                Rule::Map => "mapname".into(),
                Rule::NeedPass => "g_needpass".into(),
                Rule::ServerName => "sv_hostname".into(),
            },
        }
    }
}

impl Protocol for ProtocolImpl {
    fn make_request(&self, _state: Option<Value>) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetStatus(q3a::GetStatusData {
            challenge: "RGS".into(),
        }).write_bytes(&mut out)
            .unwrap();
        out
    }

    fn parse_response(&self, p: Packet) -> ProtocolResultStream {
        Box::new(
            futures::stream::iter_result(vec![
                q3a::Packet::from_bytes(p.data.as_slice().into())
                    .map_err(|e| format_err!("{}", e))
                    .and_then(|(_, pkt)| match pkt {
                        q3a::Packet::InfoResponse(pkt) => {
                            let mut server = Server::new(p.addr);

                            parse_q3a_server(&mut server, pkt, self.rule_names.clone())?;

                            Ok(ParseResult::Output(server))
                        }
                        other => Err(format_err!("Wrong packet type: {:?}", other.get_type())
                            .context(Error::DataParseError)
                            .into()),
                    }),
            ]).map_err({
                let p = p.clone();
                move |e| (Some(p.clone()), e)
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_server() {
        let addr = "77.93.223.201:27960".parse().unwrap();
        let _fixture = Packet {
            addr,
            data: include_bytes!("test_payload/q3s_response.raw").to_vec(),
        };
    }
}
