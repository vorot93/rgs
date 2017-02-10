extern crate std;
extern crate serde_json;
extern crate rgs_models as models;

use std::fmt::Debug;

use errors::Error;
use std::sync::{Arc, Mutex};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug)]
pub enum ProtocolRelationship {
    Current,
    Child,
}

pub type Config = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Debug)]
pub struct Packet {
    pub addr: std::net::SocketAddr,
    pub data: Vec<u8>,
}

pub trait Protocol {
    fn make_request(&self, &Config) -> Result<Vec<u8>, Error>;
    fn parse_response<'a>
        (&self,
         &Packet,
         &Config)
         -> Result<(Vec<models::Server>, Vec<(ProtocolRelationship, std::net::SocketAddr)>), Error>;
}

trait_alias!(SProtocol = Protocol + Send + Sync + Debug);

#[derive(Debug)]
pub struct ProtocolConfigEntry {
    pub data: Box<SProtocol>,
    pub config: Map<String, Value>,
    pub child: Option<Arc<Mutex<ProtocolConfigEntry>>>,
}

impl ProtocolConfigEntry {
    pub fn make<T: SProtocol + Default + 'static>() -> Arc<Mutex<ProtocolConfigEntry>> {
        Arc::new(Mutex::new(ProtocolConfigEntry {
            data: Box::new(T::default()),
            config: Map::default(),
            child: Option::default(),
        }))
    }
}

pub type TProtocolEntry = Arc<Mutex<ProtocolConfigEntry>>;

pub type ProtocolConfig = std::collections::HashMap<String, TProtocolEntry>;
