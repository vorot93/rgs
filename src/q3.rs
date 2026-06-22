use crate::model::{Player, Server, ServerFilter};
use anyhow::format_err;
use bimap::BiMap;
use q3a::MasterQueryExtra::*;
use serde_json::Value;
use std::{
    collections::{HashMap, hash_map::Entry},
    net::{SocketAddr, SocketAddrV4},
    str::FromStr,
    sync::LazyLock,
};

/// Server metadata fields that map to q3 rule names.
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

/// The standard Quake 3 rule-name mapping.
pub fn default_rule_names() -> &'static BiMap<Rule, String> {
    static V: LazyLock<BiMap<Rule, String>> = LazyLock::new(|| {
        [
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
        .collect()
    });

    LazyLock::force(&V)
}

/// The `getstatus` request datagram sent to a single server.
pub fn make_getstatus() -> Vec<u8> {
    let mut out = Vec::new();
    q3a::Packet::GetStatus(q3a::GetStatusData {
        challenge: "RGS".into(),
    })
    .write_bytes(&mut out)
    .unwrap();
    out
}

/// The `getservers` request datagram sent to a master server.
pub fn make_getservers(version: u32) -> Vec<u8> {
    let mut out = Vec::new();
    q3a::Packet::GetServers(q3a::GetServersData {
        request_tag: None,
        version,
        extra: vec![Empty, Full].into_iter().collect(),
    })
    .write_bytes(&mut out)
    .unwrap();
    out
}

fn apply_rules(srv: &mut Server, pkt: q3a::StatusResponseData, rule_names: &BiMap<Rule, String>) {
    use self::Rule::*;

    let mut rules = pkt.info;

    fn f<'a, T: FromStr>(
        v: &mut Option<T>,
        rule_names: &'a BiMap<Rule, String>,
        rules: &'a mut HashMap<String, String>,
        rule: Rule,
        transform: fn(&str) -> Option<T>,
    ) {
        if let Some(rule_name) = rule_names.get_by_left(&rule)
            && let Entry::Occupied(rule) = rules.entry(rule_name.clone())
            && let Some(transformed) = (transform)(rule.get())
        {
            *v = Some(transformed);
            rule.remove();
        }
    }

    fn s_transform(v: &str) -> Option<String> {
        Some(v.to_string())
    }
    fn b_transform(v: &str) -> Option<bool> {
        match v {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        }
    }

    (f)(
        &mut srv.name,
        rule_names,
        &mut rules,
        ServerName,
        s_transform,
    );
    (f)(&mut srv.mod_name, rule_names, &mut rules, Mod, s_transform);
    (f)(
        &mut srv.game_type,
        rule_names,
        &mut rules,
        GameType,
        s_transform,
    );
    (f)(&mut srv.map, rule_names, &mut rules, Map, s_transform);
    (f)(&mut srv.secure, rule_names, &mut rules, Secure, b_transform);
    (f)(
        &mut srv.need_pass,
        rule_names,
        &mut rules,
        NeedPass,
        b_transform,
    );
    (f)(
        &mut srv.max_clients,
        rule_names,
        &mut rules,
        MaxClients,
        |v| v.parse().ok(),
    );
    srv.num_clients = Some(pkt.players.len() as u64);
    srv.players = Some(pkt.players.into_iter().map(From::from).collect());

    srv.rules = rules
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
}

/// Parse a `statusResponse` datagram into a [`Server`], applying the rule mapping
/// and the filter. Returns `Ok(None)` when the filter drops the server.
pub fn parse_status_response(
    addr: SocketAddr,
    data: &[u8],
    rule_names: &BiMap<Rule, String>,
    filter: &ServerFilter,
) -> anyhow::Result<Option<Server>> {
    let (_, pkt) = q3a::Packet::from_bytes(data).map_err(|e| format_err!("{e:?}"))?;
    match pkt {
        q3a::Packet::StatusResponse(pkt) => {
            let mut server = Server::new(addr);
            apply_rules(&mut server, pkt, rule_names);
            Ok((filter.0)(server))
        }
        other => Err(format_err!("Wrong packet type: {:?}", other.get_type())),
    }
}

/// Parse a `getserversResponse` datagram into its server addresses and the `eot`
/// (end-of-transmission) flag marking the master's final datagram.
pub fn parse_getservers_response(data: &[u8]) -> anyhow::Result<(Vec<SocketAddrV4>, bool)> {
    let (_, pkt) = q3a::Packet::from_bytes(data).map_err(|e| format_err!("{e:?}"))?;
    match pkt {
        q3a::Packet::GetServersResponse(resp) => Ok((resp.data.into_iter().collect(), resp.eot)),
        other => Err(format_err!("Wrong packet type: {:?}", other.get_type())),
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

    fn getservers_response_bytes(addrs: &[SocketAddrV4], eot: bool) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
            data: addrs.iter().copied().collect(),
            eot,
        })
        .write_bytes(&mut out)
        .unwrap();
        out
    }

    #[test]
    fn make_getstatus_builds_getstatus_with_challenge() {
        let bytes = make_getstatus();
        let (_, pkt) = q3a::Packet::from_bytes(&bytes).unwrap();
        match pkt {
            q3a::Packet::GetStatus(data) => assert_eq!(data.challenge.trim(), "RGS"),
            other => panic!("expected GetStatus, got {:?}", other.get_type()),
        }
    }

    #[test]
    fn make_getservers_includes_version_and_flags() {
        let bytes = make_getservers(68);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("getservers"), "request: {text:?}");
        assert!(text.contains("68"), "request: {text:?}");
        assert!(text.contains("empty"), "request: {text:?}");
        assert!(text.contains("full"), "request: {text:?}");
    }

    #[test]
    fn parse_status_response_maps_rules_and_players() {
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

        let server = parse_status_response(
            addr(),
            &data,
            default_rule_names(),
            &ServerFilter::default(),
        )
        .unwrap()
        .expect("filter kept the server");

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

        assert_eq!(
            server.rules.get("custom_rule"),
            Some(&Value::String("42".to_string()))
        );
    }

    #[test]
    fn parse_status_response_rejects_wrong_packet_type() {
        let data = make_getstatus();
        assert!(
            parse_status_response(
                addr(),
                &data,
                default_rule_names(),
                &ServerFilter::default()
            )
            .is_err()
        );
    }

    #[test]
    fn parse_status_response_filter_can_drop_servers() {
        let filter = ServerFilter(std::sync::Arc::new(|_| None) as crate::model::ServerFilterFunc);
        let data = status_response_bytes(&[("sv_hostname", "Filtered")], vec![]);
        let kept = parse_status_response(addr(), &data, default_rule_names(), &filter).unwrap();
        assert!(kept.is_none());
    }

    #[test]
    fn parse_getservers_response_returns_addrs_and_eot() {
        use std::net::Ipv4Addr;
        let addrs = [
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960),
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 27961),
        ];
        let (parsed, eot) =
            parse_getservers_response(&getservers_response_bytes(&addrs, true)).unwrap();
        assert!(eot);
        let got: std::collections::HashSet<SocketAddrV4> = parsed.into_iter().collect();
        let want: std::collections::HashSet<SocketAddrV4> = addrs.into_iter().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn parse_getservers_response_partial_clears_eot() {
        use std::net::Ipv4Addr;
        let addrs = [SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960)];
        let (_, eot) =
            parse_getservers_response(&getservers_response_bytes(&addrs, false)).unwrap();
        assert!(!eot);
    }

    #[test]
    fn parse_getservers_response_rejects_wrong_packet_type() {
        let data = make_getservers(68);
        assert!(parse_getservers_response(&data).is_err());
    }
}
