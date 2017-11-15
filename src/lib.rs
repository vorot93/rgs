extern crate log;
extern crate rand;
extern crate resolve;
#[macro_use]
extern crate serde_json;
extern crate rgs_models as models;
#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate error_chain;

extern crate futures;
#[macro_use]
extern crate tokio_core;

use std::{env, io};
use std::net::SocketAddr;

use futures::{Future, Poll};
use tokio_core::net::UdpSocket;
use tokio_core::reactor::Core;

use protocols::models as pmodels;
use errors::Error;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

#[macro_use]
pub mod errors;
#[macro_use]
pub mod util;
pub mod protocols;


#[derive(Clone, Debug)]
pub struct ServerEntry {
    protocol: pmodels::TProtocol,
    data: models::Server,
}

impl std::hash::Hash for ServerEntry {
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
    fn new(protocol: pmodels::TProtocol, data: models::Server) -> ServerEntry {
        ServerEntry {
            protocol: protocol,
            data: data,
        }
    }

    fn into_inner(self) -> (pmodels::TProtocol, models::Server) {
        (self.protocol, self.data)
    }
}

struct QueryServer {
    socket: UdpSocket,
    buf: Vec<u8>,
    to_send: Option<(usize, SocketAddr)>,
}

impl Future for QueryServer {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        loop {
            // First we check to see if there's a message we need to echo back.
            // If so then we try to send it back to the original source, waiting
            // until it's writable and we're able to do so.
            if let Some((size, peer)) = self.to_send {
                let amt = try_nb!(self.socket.send_to(&self.buf[..size], &peer));
                println!("Echoed {}/{} bytes to {}", amt, size, peer);
                self.to_send = None;
            }

            // If we're here then `to_send` is `None`, so we take a look for the
            // next message we're going to echo back.
            self.to_send = Some(try_nb!(self.socket.recv_from(&mut self.buf)));
        }
    }
}

pub struct DnsResolver {
    dns: resolve::resolver::DnsResolver,
}

impl DnsResolver {
    pub fn new() -> errors::Result<DnsResolver> {
        let mut dnsconf = resolve::config::default_config()?;
        dnsconf.timeout = std::time::Duration::new(5, 0);
        Ok(Self {
            dns: resolve::resolver::DnsResolver::new(dnsconf)?,
        })
    }
}

pub trait QueryService {
    fn query_udp(
        &self,
        req: Vec<protocols::models::QueryEntry>,
    ) -> Result<std::collections::HashSet<ServerEntry>, Error>;
}

pub struct RealQueryManager {
    logger: Arc<util::LoggingService + Send + Sync + 'static>,
    dns_resolver: Arc<DnsResolver>,
    output_sink: Arc<io::Write>,
}

impl RealQueryManager {
    pub fn new<LOG, W>(logger: LOG, output_sink: W) -> RealQueryManager
    where
        LOG: util::LoggingService + Send + Sync + 'static,
        W: io::Write + Send + Sync + 'static,
    {
        RealQueryManager {
            logger: Arc::new(logger),
            dns_resolver: Arc::new(DnsResolver::new().unwrap()),
            output_sink: Arc::new(output_sink),
        }
    }
}


impl QueryService for RealQueryManager {
    fn query_udp(
        &self,
        req: Vec<pmodels::QueryEntry>,
    ) -> errors::Result<HashSet<ServerEntry>> {
        let res = Arc::<HashSet<ServerEntry>>::default();

        let queried_servers = Arc::new(HashMap::<SocketAddr, pmodels::TProtocol>::default());

        // Initialize Tokio server
        let addr = "0.0.0.0:5678".parse()?;
        let mut l = Core::new().unwrap();
        let handle = l.handle();
        let socket = UdpSocket::bind(&addr, &handle).unwrap();
        l.run(QueryServer {
            socket,
            buf: vec![0; 1024],
            to_send: None,
        })?;
        Ok(Arc::try_unwrap(res).unwrap())
    }
}
