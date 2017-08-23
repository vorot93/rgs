extern crate std;
extern crate serde_json;
extern crate rgs_models as models;

use std::fmt::Debug;

use errors::Error;
use std::sync::{Arc, Mutex};
use serde_json::{Map, Value};

pub type Config = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Debug)]
pub struct Packet {
    pub addr: std::net::SocketAddr,
    pub data: Vec<u8>,
}

pub type RequestFunc = fn(&Config) -> Result<Vec<u8>, Error>;
pub type ResponseFunc = fn(&Packet,
                           &Config,
                           Arc<Mutex<Protocol>>,
                           Option<Arc<Mutex<Protocol>>>)
                           -> Result<
    (Vec<models::Server>,
     Vec<(Arc<Mutex<Protocol>>, std::net::SocketAddr)>),
    Error,
>;

pub fn RequestDummy(_: &Config) -> Result<Vec<u8>, Error> {
    unimplemented!()
}

pub fn ResponseDummy(
    _: &Packet,
    _: &Config,
    _: Arc<Mutex<Protocol>>,
    _: Option<Arc<Mutex<Protocol>>>,
) -> Result<(Vec<models::Server>, Vec<(Arc<Mutex<Protocol>>, std::net::SocketAddr)>), Error> {
    unimplemented!()
}

pub struct Protocol {
    pub config: Config,
    pub child: Option<Arc<Mutex<Protocol>>>,
    pub make_request_fn: RequestFunc,
    pub parse_response_fn: ResponseFunc,
}

impl Default for Protocol {
    fn default() -> Protocol {
        Protocol {
            config: Config::default(),
            child: None,
            make_request_fn: RequestDummy,
            parse_response_fn: ResponseDummy,
        }
    }
}

impl Debug for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        self.config.fmt(f)?;
        self.child.fmt(f)?;

        Ok(())
    }
}

pub type TProtocol = Arc<Mutex<Protocol>>;
pub type ProtocolConfig = std::collections::HashMap<String, TProtocol>;
