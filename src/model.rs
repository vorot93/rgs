use crate::protocol::Protocol;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};

/// A request to query one host with one protocol.
#[derive(Clone, Debug)]
pub struct Query {
    pub protocol: Arc<dyn Protocol>,
    pub host: Host,
}

/// Where to send a query: a resolved socket address, or a name + port to resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    Addr(SocketAddr),
    Named { host: String, port: u16 },
}

impl From<SocketAddr> for Host {
    fn from(addr: SocketAddr) -> Self {
        Host::Addr(addr)
    }
}

impl<S> From<(S, u16)> for Host
where
    S: ToString,
{
    fn from((host, port): (S, u16)) -> Self {
        Host::Named {
            host: host.to_string(),
            port,
        }
    }
}

/// A queried server together with the protocol that produced it.
#[derive(Clone, Debug)]
pub struct ServerEntry {
    pub protocol: Arc<dyn Protocol>,
    pub server: Server,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub ping: Option<i64>,
    pub info: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub addr: SocketAddr,

    #[serde(default)]
    pub rules: BTreeMap<String, Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub need_pass: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_clients: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_clients: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_bots: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping: Option<Duration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub players: Option<Vec<Player>>,
}

impl Server {
    pub fn new(addr: SocketAddr) -> Server {
        Server {
            addr,
            rules: BTreeMap::new(),
            name: None,
            need_pass: None,
            mod_name: None,
            game_type: None,
            map: None,
            num_clients: None,
            max_clients: None,
            num_bots: None,
            secure: None,
            ping: None,
            players: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> SocketAddr {
        "127.0.0.1:27960".parse().unwrap()
    }

    #[test]
    fn host_from_conversions() {
        assert_eq!(Host::from(addr()), Host::Addr(addr()));
        assert_eq!(
            Host::from(("example.com", 27960)),
            Host::Named {
                host: "example.com".to_string(),
                port: 27960,
            }
        );
    }

    #[test]
    fn server_skips_none_optionals_when_serializing() {
        let value = serde_json::to_value(Server::new(addr())).unwrap();
        let obj = value.as_object().unwrap();

        // Always-present fields.
        assert!(obj.contains_key("addr"));
        assert!(obj.contains_key("rules"));

        // Optional fields left as None must be omitted entirely.
        for key in ["name", "need_pass", "map", "num_clients", "ping", "players"] {
            assert!(!obj.contains_key(key), "expected `{key}` to be skipped");
        }
    }

    #[test]
    fn server_roundtrips_optional_fields() {
        let mut srv = Server::new(addr());
        srv.name = Some("Test Server".to_string());
        srv.map = Some("q3dm6".to_string());
        srv.ping = Some(Duration::from_millis(42));
        srv.players = Some(vec![Player {
            name: "player1".to_string(),
            ping: Some(20),
            ..Default::default()
        }]);

        let roundtripped: Server =
            serde_json::from_value(serde_json::to_value(srv.clone()).unwrap()).unwrap();

        assert_eq!(roundtripped, srv);
    }
}
