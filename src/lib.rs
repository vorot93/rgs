#![feature(proc_macro)]
#![feature(conservative_impl_trait)]
#![feature(generators)]
#![feature(trait_alias)]

#[macro_use]
extern crate enum_primitive_derive;
#[macro_use]
extern crate failure;
extern crate futures_await as futures;
extern crate log;
extern crate num_traits;
extern crate rand;
extern crate resolve;
extern crate rgs_models as models;
#[macro_use]
extern crate serde_json;
extern crate tokio_core;
extern crate tokio_dns;
extern crate tokio_timer;

use errors::Error;
use models::Server;
use std::{env, io};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use futures::prelude::*;
use futures::sync::mpsc::{Receiver, Sender};
use std::net::SocketAddr;
use std::time::Duration;
use protocols::models as pmodels;
use tokio_core::net::UdpSocket;
use tokio_timer::Timer;

#[macro_use]
pub mod errors;
#[macro_use]
pub mod util;
pub mod protocols;

#[derive(Clone, Debug)]
pub struct ServerEntry {
    protocol: pmodels::TProtocol,
    data: Server,
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
    pub fn new(protocol: pmodels::TProtocol, data: models::Server) -> ServerEntry {
        ServerEntry {
            protocol: protocol,
            data: data,
        }
    }

    pub fn into_inner(self) -> (pmodels::TProtocol, models::Server) {
        (self.protocol, self.data)
    }
}

pub trait ProtocolResolver: Sink<SinkItem = pmodels::Query, SinkError = Error> {
    fn resolve_protocol(&self, addr: SocketAddr) -> Option<pmodels::TProtocol>;
}

pub struct RealParserService {
    data_pipe: (
        Box<Stream<Item = pmodels::Packet, Error = Error>>,
        Arc<Sink<SinkItem = pmodels::Packet, SinkError = Error>>,
    ),
    user_input: Box<Stream<Item = pmodels::Query, Error = Error>>,
}

impl RealParserService {
    fn new<F: Stream<Item = pmodels::Query, Error = Error>>(f: F) -> Self {
        let (data_in, data_out) = futures::sync::mpsc::channel(1);
        Self {
            data_pipe: (
                Box::new(data_out.map_err(|_| Error::NetworkError { what: "".into() })),
                Arc::new(data_in.sink_map_err(|_| Error::NetworkError { what: "".into() })),
            ),
            user_input: f,
        }
    }
}

pub enum ParseResult {
    FollowUp(pmodels::Query),
    Output(ServerEntry),
}

pub trait ParserService {
    fn build(self) -> Box<Stream<Item = ParseResult, Error = Error>>;
}

impl ParserService for RealParserService {
    #[async_stream(boxed, item = ParseResult)]
    fn build(self) -> errors::Result<()> {
        let (data_input, data_output) = self.data_pipe;

        let (follow_up_in, follow_up_out) = futures::sync::mpsc::channel::<pmodels::Query>(1);
        let mut follow_up_in = Box::new(follow_up_in);
        let query_sink = self.user_input
            .chain(follow_up_out.map_err(|e| Error::NetworkError { what: format!("") }));

        let protocol =
            Arc::new(protocols::openttds::Protocol::new(&pmodels::Config::new()).unwrap());
        #[async]
        for pkt in data_input {
            if let Ok(parse_result) = pmodels::Protocol::parse_response(&*protocol, &pkt) {
                for follow_up_entry in parse_result.follow_up {
                    stream_yield!(ParseResult::FollowUp(follow_up_entry));
                }
                for server in parse_result.servers {
                    stream_yield!(ParseResult::Output(ServerEntry {
                        protocol: protocol.clone(),
                        data: server,
                    }));
                }
            }
        }

        Ok(())
    }
}

#[async]
fn dns_resolve(
    resolver: Arc<tokio_dns::Resolver>,
    host: pmodels::Host,
) -> Result<SocketAddr, Error> {
    match host {
        pmodels::Host::A(addr) => Ok(addr),
        pmodels::Host::S(stringaddr) => await!(resolver.resolve(&stringaddr.host))
            .map_err(|e| Error::NetworkError {
                what: std::error::Error::description(&e).into(),
            })?
            .into_iter()
            .next()
            .map(|ipaddr| SocketAddr::new(ipaddr, stringaddr.port))
            .ok_or_else(|| Error::NetworkError {
                what: format!("Failed to resolve host {}", &stringaddr.host),
            }),
    }
}

pub trait Querier {
    fn query(
        self,
        user_input: Box<Stream<Item = protocols::models::Query, Error = Error>>,
    ) -> Box<Stream<Error = Error, Item = ServerEntry> + 'static>;
}

pub struct UDPQuery {
    socket: Arc<UdpSocket>,
    dns_resolver: Arc<tokio_dns::Resolver>,

    parser_stream: Arc<Stream<Item = ParseResult, Error = errors::Error>>,
    send_queue_in: futures::sync::mpsc::Sender<pmodels::Query>,
    send_queue_out: futures::sync::mpsc::Receiver<pmodels::Query>,
}

impl UDPQuery {
    fn new<P: ParserService>(parser: P, socket: UdpSocket) -> Self {
        Self {
            parser_stream: Arc::new(parser.build()),
            socket: Arc::new(socket),
        }
    }
}

impl Sink for UDPQuery {
    type SinkItem = pmodels::Query;
    type SinkError = Error;

    fn start_send(&mut self, v: pmodels::Query) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.send_queue_in.start_send(v)
    }

    fn poll_complete(&mut self) -> Poll<(), Error> {
        self.send_queue_in.poll_complete()
    }

    fn close(&mut self) -> Poll<(), Error> {
        self.send_queue_in.close()
    }
}

#[async]
fn udp_send(
    socket: Arc<UdpSocket>,
    dns_resolver: Arc<tokio_dns::Resolver>,
    addr: pmodels::Host,
    data: Vec<u8>,
) -> errors::Result<()> {
    if let Ok(addr) = await!(dns_resolve(dns_resolver.clone(), addr)) {
        await!(socket.send_dgram(data, addr));
    }

    Ok(())
}

impl Stream for UDPQuery {
    type Item = ServerEntry;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Async::Ready(Some(query)) = self.send_queue_out.poll().unwrap() {
            let data = query.protocol.make_request();
            async_block ! {
                await!(udp_send(self.socket.clone(), self.dns_resolver.clone(), query.addr, data))
            };
        }

        if let Async::Ready(v) = self.parser_stream.poll()? {
            match v {
                Some(data) => match data {
                    ParseResult::FollowUp(s) => {
                        self.send_queue_in.try_send(s).unwrap();
                    }
                    ParseResult::Output(s) => {
                        return Ok(Async::Ready(Some(s)));
                    }
                },
                None => {
                    return Ok(Async::Ready(None));
                }
            }
        }

        Ok(Async::NotReady)
    }
}

pub struct UDPQueryService {
    parser: Box<ParserService>,
    socket: UdpSocket,
}

impl UDPQueryService {
    fn new() {}

    fn into_query(
        self,
        user_input: Box<Stream<Item = protocols::models::Query, Error = Error>>,
    ) -> UDPQuery {
        UDPQuery::new(self.parser, self.socket)
    }
}
