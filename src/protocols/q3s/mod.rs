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
