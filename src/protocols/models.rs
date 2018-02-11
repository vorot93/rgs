extern crate futures_await as futures;
extern crate iso_country;
extern crate serde;
extern crate serde_json;
extern crate std;

use errors::Error;
use futures::prelude::*;
use std::sync::Arc;
use self::iso_country::Country as CountryBase;
use std::ops::Deref;
use std::str::FromStr;
use self::serde::*;
use self::serde::de::{Deserializer, Visitor};
use serde_json::Value;

pub type Config = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Debug)]
pub struct Packet {
    pub addr: std::net::SocketAddr,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringAddr {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    A(std::net::SocketAddr),
    S(StringAddr),
}

#[derive(Clone, Debug)]
pub struct UserQuery {
    pub protocol: TProtocol,
    pub host: Host,
}

impl PartialEq for UserQuery {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host && Arc::ptr_eq(&self.protocol, &other.protocol)
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

/// Protocol defines a common way to communicate with queried servers of a single type.
pub trait Protocol: std::fmt::Debug + Send + Sync {
    /// Creates a request packet. Can accept an optional state if there is any.
    fn make_request(&self, state: Option<Value>) -> Vec<u8>;
    /// Create a stream of parsed values out of incoming response.
    fn parse_response(&self, p: Packet) -> Box<Stream<Item = ParseResult, Error = Error>>;
}

#[derive(Clone, Debug)]
pub struct TProtocol {
    pub inner: Arc<Protocol>,
}

impl std::ops::Deref for TProtocol {
    type Target = Arc<Protocol>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl PartialEq for TProtocol {
    fn eq(&self, other: &TProtocol) -> bool {
        Arc::ptr_eq(&*self, other)
    }
}

pub type ProtocolConfig = std::collections::HashMap<String, TProtocol>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Country(CountryBase);

impl Deref for Country {
    type Target = iso_country::Country;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
        serializer.serialize_str(self.to_string().as_str())
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Unspecified,
    Up,
    Down,
}

impl Default for Status {
    fn default() -> Status {
        Status::Unspecified
    }
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
    pub rules: serde_json::Map<String, Value>,

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
    pub terrain: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_clients: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_clients: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_bots: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub players: Option<Vec<Player>>,
}

impl Server {
    pub fn new(addr: std::net::SocketAddr) -> Server {
        Server {
            addr: addr,
            status: Status::default(),
            country: Country::default(),
            rules: serde_json::Map::default(),
            name: Option::default(),
            need_pass: Option::default(),
            mod_name: Option::default(),
            game_type: Option::default(),
            terrain: Option::default(),
            num_clients: Option::default(),
            max_clients: Option::default(),
            num_bots: Option::default(),
            secure: Option::default(),
            ping: Option::default(),
            players: Option::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate serde_json;

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

        let result = serde_json::to_value(&fixture).unwrap();

        assert_eq!(expectation, result);
    }

    #[test]
    fn deserialization() {
        let (fixture, expectation) = fixtures();

        let result = serde_json::from_value(fixture).unwrap();

        assert_eq!(expectation, result);
    }
}
