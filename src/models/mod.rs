use derive_more::From;
use futures::stream::BoxStream;
use iso_country::Country as CountryBase;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Visitor},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    net::SocketAddr,
    ops::Deref,
    str::FromStr,
    string::ToString,
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct ServerEntry {
    pub protocol: TProtocol,
    pub data: Server,
}

impl Hash for ServerEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.addr.hash(state)
    }
}

impl PartialEq for ServerEntry {
    fn eq(&self, other: &ServerEntry) -> bool {
        self.data.addr == other.data.addr
    }
}

impl Eq for ServerEntry {}

impl ServerEntry {
    pub fn new(protocol: TProtocol, data: Server) -> ServerEntry {
        ServerEntry { protocol, data }
    }

    pub fn into_inner(self) -> (TProtocol, Server) {
        (self.protocol, self.data)
    }
}

pub type Config = HashMap<String, Value>;

#[derive(Clone, Debug)]
pub struct Packet {
    pub addr: SocketAddr,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringAddr {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    A(SocketAddr),
    S(StringAddr),
}

impl From<SocketAddr> for Host {
    fn from(addr: SocketAddr) -> Self {
        Host::A(addr)
    }
}

impl<S> From<(S, u16)> for Host
where
    S: ToString,
{
    fn from((host, port): (S, u16)) -> Self {
        Host::S(StringAddr {
            host: host.to_string(),
            port,
        })
    }
}

#[derive(Clone, Debug)]
pub struct UserQuery {
    pub protocol: TProtocol,
    pub host: Host,
}

impl PartialEq for UserQuery {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host
            && std::ptr::addr_eq(Arc::as_ptr(&self.protocol), Arc::as_ptr(&other.protocol))
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

#[derive(Clone, Debug, PartialEq)]
pub struct ServerResponse {
    pub host: Host,
    pub protocol: TProtocol,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FollowUpQueryProtocol {
    This,
    Child(TProtocol),
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub enum ParseResult {
    FollowUp(FollowUpQuery),
    Output(Server),
}

pub type ProtocolResultStream =
    BoxStream<'static, Result<ParseResult, (Option<Packet>, anyhow::Error)>>;

/// Protocol defines a common way to communicate with queried servers of a single type.
pub trait Protocol: std::fmt::Debug + Send + Sync + 'static {
    /// Creates a request packet. Can accept an optional state if there is any.
    fn make_request(&self, state: Option<Value>) -> Vec<u8>;
    /// Create a stream of parsed values out of incoming response.
    fn parse_response(&self, p: Packet) -> ProtocolResultStream;
}

#[derive(Clone, Debug)]
pub struct TProtocol {
    inner: Arc<dyn Protocol>,
}

impl Deref for TProtocol {
    type Target = Arc<dyn Protocol>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl PartialEq for TProtocol {
    fn eq(&self, other: &TProtocol) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(self), Arc::as_ptr(other))
    }
}

impl<T> From<T> for TProtocol
where
    T: Protocol,
{
    fn from(v: T) -> Self {
        Self {
            inner: Arc::new(v) as Arc<dyn Protocol>,
        }
    }
}

pub type ProtocolConfig = std::collections::HashMap<String, TProtocol>;

#[derive(Clone, Debug, PartialEq, Eq, From)]
pub struct Country(pub CountryBase);

impl Default for Country {
    fn default() -> Country {
        Country(CountryBase::Unspecified)
    }
}

impl Serialize for Country {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.to_string().as_str())
    }
}

impl<'de> Deserialize<'de> for Country {
    fn deserialize<D>(deserializer: D) -> Result<Country, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CountryVisitor;
        impl<'de> Visitor<'de> for CountryVisitor {
            type Value = Country;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("ISO country code")
            }

            fn visit_str<E>(self, value: &str) -> Result<Country, E>
            where
                E: de::Error,
            {
                Ok(Country(
                    CountryBase::from_str(value).unwrap_or(CountryBase::Unspecified),
                ))
            }
        }
        deserializer.deserialize_str(CountryVisitor)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    #[default]
    Unspecified,
    Up,
    Down,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub ping: Option<i64>,
    pub info: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Server {
    // Mandatory parameters
    pub addr: std::net::SocketAddr,

    #[serde(default)]
    pub status: Status,

    #[serde(default)]
    pub country: Country,

    #[serde(default)]
    pub rules: BTreeMap<String, Value>,

    // Optional fields
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
            status: Default::default(),
            country: Default::default(),
            rules: Default::default(),
            name: Default::default(),
            need_pass: Default::default(),
            mod_name: Default::default(),
            game_type: Default::default(),
            map: Default::default(),
            num_clients: Default::default(),
            max_clients: Default::default(),
            num_bots: Default::default(),
            secure: Default::default(),
            ping: Default::default(),
            players: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixtures() -> (Value, Server) {
        let mut srv = Server::new(std::net::SocketAddr::from_str("127.0.0.1:9000").unwrap());
        srv.status = Status::Up;
        srv.country = Country(CountryBase::RU);
        srv.rules.insert("protocol-version".into(), 84.into());

        let ser = json!({
            "addr": "127.0.0.1:9000",
            "status": "Up",
            "country": "RU",
            "rules": {
                "protocol-version": 84,
            },
        });

        (ser, srv)
    }

    #[test]
    fn serialization() {
        let (expectation, fixture) = fixtures();

        let result = serde_json::to_value(fixture).unwrap();

        assert_eq!(expectation, result);
    }

    #[test]
    fn deserialization() {
        let (fixture, expectation) = fixtures();

        let result = serde_json::from_value(fixture).unwrap();

        assert_eq!(expectation, result);
    }

    #[derive(Debug)]
    struct DummyProtocol;

    impl Protocol for DummyProtocol {
        fn make_request(&self, _: Option<Value>) -> Vec<u8> {
            Vec::new()
        }
        fn parse_response(&self, _: Packet) -> ProtocolResultStream {
            Box::pin(futures::stream::empty())
        }
    }

    fn addr() -> SocketAddr {
        SocketAddr::from_str("127.0.0.1:27960").unwrap()
    }

    #[test]
    fn country_serializes_to_iso_code() {
        assert_eq!(
            serde_json::to_value(Country(CountryBase::RU)).unwrap(),
            json!("RU")
        );
        // The unspecified country has no ISO code, so it serializes to an empty string.
        assert_eq!(serde_json::to_value(Country::default()).unwrap(), json!(""));
    }

    #[test]
    fn country_deserializes_known_and_unknown_codes() {
        let known: Country = serde_json::from_value(json!("RU")).unwrap();
        assert_eq!(known, Country(CountryBase::RU));

        // Unknown / garbage codes fall back to Unspecified rather than erroring.
        let unknown: Country = serde_json::from_value(json!("not-a-code")).unwrap();
        assert_eq!(unknown, Country::default());
    }

    #[test]
    fn host_from_conversions() {
        assert_eq!(Host::from(addr()), Host::A(addr()));

        assert_eq!(
            Host::from(("example.com", 27960)),
            Host::S(StringAddr {
                host: "example.com".to_string(),
                port: 27960,
            })
        );
    }

    #[test]
    fn query_userquery_roundtrip() {
        let protocol = TProtocol::from(DummyProtocol);
        let user = UserQuery {
            protocol: protocol.clone(),
            host: Host::A(addr()),
        };

        let query = Query::from(user.clone());
        assert_eq!(query.host, Host::A(addr()));
        assert!(query.state.is_none());

        // Converting back drops the state and reproduces the original UserQuery.
        assert_eq!(UserQuery::from(query), user);
    }

    #[test]
    fn tprotocol_equality_is_pointer_identity() {
        let a = TProtocol::from(DummyProtocol);
        let b = a.clone();
        let c = TProtocol::from(DummyProtocol);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn userquery_equality_uses_host_and_protocol_identity() {
        let protocol = TProtocol::from(DummyProtocol);
        let other_protocol = TProtocol::from(DummyProtocol);

        let base = UserQuery {
            protocol: protocol.clone(),
            host: Host::A(addr()),
        };

        assert_eq!(
            base,
            UserQuery {
                protocol: protocol.clone(),
                host: Host::A(addr()),
            }
        );

        // Different protocol instance => not equal.
        assert_ne!(
            base,
            UserQuery {
                protocol: other_protocol,
                host: Host::A(addr()),
            }
        );

        // Different host => not equal.
        assert_ne!(
            base,
            UserQuery {
                protocol,
                host: Host::from(("example.com", 27960)),
            }
        );
    }

    #[test]
    fn server_skips_none_optionals_when_serializing() {
        let value = serde_json::to_value(Server::new(addr())).unwrap();
        let obj = value.as_object().unwrap();

        // Mandatory fields are always present.
        assert!(obj.contains_key("addr"));
        assert!(obj.contains_key("status"));
        assert!(obj.contains_key("country"));
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
