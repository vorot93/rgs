use crate::{
    model::{Player, Server},
    protocol::{Outcome, Parsed, Protocol},
};
use anyhow::format_err;
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    net::SocketAddr,
    sync::Arc,
};

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
        srv.max_clients = rules.remove(rule).and_then(|v| v.parse().ok());
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

#[derive(Clone)]
pub struct ServerFilter(pub ServerFilterFunc);

impl From<ServerFilterFunc> for ServerFilter {
    fn from(f: ServerFilterFunc) -> Self {
        ServerFilter(f)
    }
}

impl Debug for ServerFilter {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "<ServerFilter>")
    }
}

/// Quake III Arena server protocol implementation.
#[derive(Debug)]
pub struct Q3s {
    /// Mapping between rule names and metadata fields.
    pub rule_names: HashMap<Rule, String>,
    /// Filter for sorting out data if the master is shared.
    pub server_filter: ServerFilter,
}

impl Default for Q3s {
    fn default() -> Self {
        Self {
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
            server_filter: ServerFilter::from(Arc::new(Some) as ServerFilterFunc),
        }
    }
}

impl Protocol for Q3s {
    fn make_request(&self) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetStatus(q3a::GetStatusData {
            challenge: "RGS".into(),
        })
        .write_bytes(&mut out)
        .unwrap();
        out
    }

    fn parse_response(&self, addr: SocketAddr, data: &[u8]) -> anyhow::Result<Parsed> {
        let (_, pkt) = q3a::Packet::from_bytes(data).map_err(|e| format_err!("{e:?}"))?;
        match pkt {
            q3a::Packet::StatusResponse(pkt) => {
                let mut server = Server::new(addr);
                parse_q3a_server(&mut server, pkt, &self.rule_names)?;
                let outcomes = (self.server_filter.0)(server)
                    .map(Outcome::Server)
                    .into_iter()
                    .collect();
                Ok(Parsed {
                    outcomes,
                    expect_more: false,
                })
            }
            other => Err(format_err!("Wrong packet type: {:?}", other.get_type())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn make_request_builds_getstatus_with_challenge() {
        let bytes = Q3s::default().make_request();
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

        let parsed = Q3s::default().parse_response(addr(), &data).unwrap();
        assert!(!parsed.expect_more);
        assert_eq!(parsed.outcomes.len(), 1);

        let server = match &parsed.outcomes[0] {
            Outcome::Server(s) => s,
            other => panic!("expected Server, got {other:?}"),
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
        let data = Q3s::default().make_request();
        assert!(Q3s::default().parse_response(addr(), &data).is_err());
    }

    #[test]
    fn server_filter_can_drop_servers() {
        let proto = Q3s {
            server_filter: ServerFilter::from(Arc::new(|_| None) as ServerFilterFunc),
            ..Default::default()
        };
        let data = status_response_bytes(&[("sv_hostname", "Filtered")], vec![]);
        let parsed = proto.parse_response(addr(), &data).unwrap();
        assert!(parsed.outcomes.is_empty());
    }
}
