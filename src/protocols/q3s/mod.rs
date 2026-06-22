use crate::{
    errors::Error,
    models::{Packet, ParseResult, Player, Protocol, ProtocolResultStream, Server},
};
use anyhow::format_err;
use derive_more::From;
use futures::stream::empty;
use q3a;
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    sync::Arc,
};
use tokio_stream::once;

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

impl From<q3a::Player> for Player {
    fn from(v: q3a::Player) -> Self {
        Self {
            name: v.name,
            ping: Some(i64::from(v.ping)),
            info: vec![("score".to_string(), Value::Number(v.score.into()))]
                .into_iter()
                .collect(),
        }
    }
}

fn parse_q3a_server(
    srv: &mut Server,
    pkt: q3a::StatusResponseData,
    rule_mapping: &HashMap<Rule, String>,
) -> anyhow::Result<()> {
    use self::Rule::*;

    let mut rules = pkt.info;

    if let Some(rule) = rule_mapping.get(&ServerName) {
        srv.name = rules.remove(rule);
    }

    if let Some(rule) = rule_mapping.get(&Mod) {
        srv.mod_name = rules.remove(rule);
    }

    if let Some(rule) = rule_mapping.get(&GameType) {
        srv.game_type = rules.remove(rule);
    }

    if let Some(rule) = rule_mapping.get(&Map) {
        srv.map = rules.remove(rule);
    }

    if let Some(rule) = rule_mapping.get(&Secure) {
        srv.secure = rules.remove(rule).map(|v| v == "1");
    }

    if let Some(rule) = rule_mapping.get(&NeedPass) {
        srv.need_pass = rules.remove(rule).map(|v| v == "1");
    }

    if let Some(rule) = rule_mapping.get(&MaxClients) {
        srv.max_clients = rules.remove(rule).and_then(|v| v.parse().ok())
    }

    srv.num_clients = Some(pkt.players.len() as u64);
    srv.players = Some(pkt.players.into_iter().map(From::from).collect());

    srv.rules = rules
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    Ok(())
}

pub type ServerFilterFunc = Arc<dyn Fn(Server) -> Option<Server> + Send + Sync + 'static>;

#[derive(Clone, From)]
pub struct ServerFilter(pub ServerFilterFunc);

impl Debug for ServerFilter {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "<ServerFilter>")
    }
}

/// Quake III Arena server protocol implementation
#[derive(Debug)]
pub struct ProtocolImpl {
    /// Version of the protocol
    pub version: u32,
    /// Mapping between rule names and metadata fields
    pub rule_names: HashMap<Rule, String>,
    /// Filter required for sorting out data if the master is shared
    pub server_filter: ServerFilter,
}

impl Default for ProtocolImpl {
    fn default() -> Self {
        Self {
            version: 71,
            rule_names: [
                (Rule::Secure, "sv_punkbuster"),
                (Rule::MaxClients, "sv_maxclients"),
                (Rule::Mod, "game"),
                (Rule::GameType, "g_gametype"),
                (Rule::Map, "mapname"),
                (Rule::NeedPass, "g_needpass"),
                (Rule::ServerName, "sv_hostname"),
            ]
            .iter()
            .map(|(rule, name)| (*rule, name.to_string()))
            .collect(),
            server_filter: From::from(Arc::new(Some) as ServerFilterFunc),
        }
    }
}

impl Protocol for ProtocolImpl {
    fn make_request(&self, _state: Option<Value>) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetStatus(q3a::GetStatusData {
            challenge: "RGS".into(),
        })
        .write_bytes(&mut out)
        .unwrap();
        out
    }

    fn parse_response(&self, p: Packet) -> ProtocolResultStream {
        match q3a::Packet::from_bytes(p.data.as_slice())
            .map_err(|e| format_err!("{:?}", e))
            .and_then(|(_, pkt)| match pkt {
                q3a::Packet::StatusResponse(pkt) => {
                    let mut server = Server::new(p.addr);

                    parse_q3a_server(&mut server, pkt, &self.rule_names)?;

                    Ok((self.server_filter.0)(server).map(ParseResult::Output))
                }
                other => Err(format_err!("Wrong packet type: {:?}", other.get_type())
                    .context(Error::DataParseError)),
            })
            .map_err({
                let p = p.clone();
                move |e| (Some(p), e)
            }) {
            Ok(opt) => match opt {
                Some(v) => Box::pin(once(Ok(v))),
                None => Box::pin(empty()),
            },
            Err(e) => Box::pin(once(Err(e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::net::SocketAddr;

    fn addr() -> SocketAddr {
        "1.2.3.4:27960".parse().unwrap()
    }

    fn status_response_bytes(info: &[(&str, &str)], players: Vec<q3a::Player>) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::StatusResponse(q3a::StatusResponseData {
            info: info
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            players,
        })
        .write_bytes(&mut out)
        .unwrap();
        out
    }

    fn parse(proto: &ProtocolImpl, data: Vec<u8>) -> Vec<ParseResult> {
        let stream = proto.parse_response(Packet { addr: addr(), data });
        futures::executor::block_on(stream.collect::<Vec<_>>())
            .into_iter()
            .map(|r| r.expect("expected a successful parse result"))
            .collect()
    }

    #[test]
    fn make_request_builds_getstatus_with_challenge() {
        let bytes = ProtocolImpl::default().make_request(None);
        let (_, pkt) = q3a::Packet::from_bytes(&bytes).unwrap();
        match pkt {
            q3a::Packet::GetStatus(data) => assert_eq!(data.challenge.trim(), "RGS"),
            other => panic!("expected GetStatus, got {:?}", other.get_type()),
        }
    }

    #[test]
    fn parse_response_maps_rules_and_players() {
        let data = status_response_bytes(
            &[
                ("sv_hostname", "My Server"),
                ("mapname", "q3dm6"),
                ("sv_maxclients", "16"),
                ("g_gametype", "0"),
                ("game", "osp"),
                ("g_needpass", "1"),
                ("sv_punkbuster", "1"),
                ("custom_rule", "42"),
            ],
            vec![q3a::Player {
                score: 10,
                ping: 25,
                name: "alice".to_string(),
            }],
        );

        let results = parse(&ProtocolImpl::default(), data);
        assert_eq!(results.len(), 1);
        let server = match &results[0] {
            ParseResult::Output(s) => s,
            other => panic!("expected Output, got {other:?}"),
        };

        assert_eq!(server.name.as_deref(), Some("My Server"));
        assert_eq!(server.map.as_deref(), Some("q3dm6"));
        assert_eq!(server.max_clients, Some(16));
        assert_eq!(server.game_type.as_deref(), Some("0"));
        assert_eq!(server.mod_name.as_deref(), Some("osp"));
        assert_eq!(server.need_pass, Some(true));
        assert_eq!(server.secure, Some(true));
        assert_eq!(server.num_clients, Some(1));

        let players = server.players.as_ref().unwrap();
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].name, "alice");
        assert_eq!(players[0].ping, Some(25));
        assert_eq!(players[0].info.get("score").unwrap(), &Value::from(10));

        // Consumed keys are removed; only unrecognised rules remain.
        assert_eq!(
            server.rules.get("custom_rule"),
            Some(&Value::String("42".to_string()))
        );
        assert!(!server.rules.contains_key("sv_hostname"));
        assert!(!server.rules.contains_key("mapname"));
    }

    #[test]
    fn parse_response_rejects_wrong_packet_type() {
        // A GetStatus packet is not a valid *response*.
        let data = ProtocolImpl::default().make_request(None);
        let stream = ProtocolImpl::default().parse_response(Packet { addr: addr(), data });
        let results = futures::executor::block_on(stream.collect::<Vec<_>>());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn server_filter_can_drop_servers() {
        let proto = ProtocolImpl {
            server_filter: ServerFilter::from(Arc::new(|_| None) as ServerFilterFunc),
            ..Default::default()
        };

        let data = status_response_bytes(&[("sv_hostname", "Filtered")], vec![]);
        assert!(parse(&proto, data).is_empty());
    }
}
