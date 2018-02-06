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
use tokio_core::net::{UdpCodec, UdpFramed, UdpSocket};
use tokio_timer::Timer;

pub mod dns;
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

type ProtocolMapping = Arc<Mutex<HashMap<SocketAddr, pmodels::TProtocol>>>;

pub enum FullParseResult {
    FollowUp(pmodels::Query),
    Output(ServerEntry),
}

trait Parser
    : Sink<SinkItem = pmodels::Packet, SinkError = Error> + Stream<Item = FullParseResult, Error = Error>
    {
}

pub struct RealParser {
    packet_sink: Sender<pmodels::Packet>,
    packet_stream: Receiver<pmodels::Packet>,
    results_stream: Box<Stream<Item = FullParseResult, Error = Error>>,
    protocol_mapping: ProtocolMapping,
}

impl RealParser {
    pub fn new(protocol_mapping: ProtocolMapping) -> Self {
        let (data_in, data_out) = futures::sync::mpsc::channel(1);
        Self {
            packet_sink: data_in.sink_map_err(|_| Error::NetworkError { what: "".into() }),
            packet_stream: data_out.map_err(|_| Error::NetworkError { what: "".into() }),
            results_stream: Box::new(futures::stream::iter_ok(vec![])),
            protocol_mapping,
        }
    }
}

impl Sink for RealParser {
    type SinkItem = pmodels::Packet;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.packet_sink.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        // TODO: protocol selection
        let protocol =
            Arc::new(protocols::openttds::Protocol::new(&pmodels::Config::new()).unwrap());

        if let Async::Ready(Some(pkt)) = self.packet_sink.poll()? {
            *self.results_stream = self.results_stream.chain(
                pmodels::Protocol::parse_response(&*protocol, &pkt).map(|v| match v {
                    pmodels::ParseResult::FollowUp(q) => FullParseResult::FollowUp(q),
                    pmodels::ParseResult::Output(s) => FullParseResult::Output(ServerEntry {
                        protocol: self.protocol_mapping.lock().unwrap().get(&s.addr).clone(),
                        data: s,
                    }),
                }),
            );
            Ok(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.packet_sink.close()
    }
}

impl Stream for RealParser {
    type Item = FullParseResult;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.results_stream.poll()
    }
}

impl Parser for RealParser {}

pub struct IncomingPacket {
    pub addr: SocketAddr,
    pub protocol: pmodels::TProtocol,
    pub data: Vec<u8>,
}

pub struct UdpQueryCodec {
    pub protocol_mapping: ProtocolMapping,
    pub dns_history: dns::History,
}

impl tokio_core::net::UdpCodec for UdpQueryCodec {
    type In = IncomingPacket;
    type Out = dns::ResolvedQuery;

    fn decode(&mut self, addr: &SocketAddr, buf: &[u8]) -> io::Result<Self::In> {
        let protocol = self.protocol_mapping
            .lock()
            .unwrap()
            .get(addr)
            .ok_or_else(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Could not determine server's protocol",
                )
            })?;
        Ok(IncomingPacket {
            protocol,
            addr: addr.clone(),
            data: buf.into(),
        })
    }

    fn encode(&mut self, v: Self::Out, into: &mut Vec<u8>) -> SocketAddr {
        let pkt = v.protocol.make_request();
        into.append(&mut v.data);
        v.addr
    }
}

/// Represents a single request by user to query the servers fed into sink.
pub struct UdpQuery {
    dns_history: dns::History,
    protocol_mapping: ProtocolMapping,

    socket_sink: Box<Sink<SinkItem = pmodels::Packet, SinkError = io::Error>>,
    socket_stream: Box<Stream<Item = pmodels::Packet, Error = Error>>,
    dns_resolver: Box<dns::Resolver>,

    parser: Box<Parser>,
    input_sink: Sender<pmodels::Query>,
    follow_up_sink: Sender<pmodels::Query>,
    query_stream: Receiver<pmodels::Query>,
}

impl UdpQuery {
    fn new<P, PF, D>(parser_builder: PF, dns_resolver: Arc<D>, socket: UdpSocket) -> Self
    where
        P: Parser,
        PF: FnOnce(ProtocolMapping) -> Box<P>,
        D: tokio_dns::Resolver + Sync + 'static,
    {
        let protocol_mapping = ProtocolMapping::default();
        let dns_history = dns::History::default();

        let (socket_sink, socket_stream) =
            socket.framed(UdpQueryCodec { protocol_mapping }).split();

        let (input_stream, input_sink) = futures::sync::mpsc::channel::<pmodels::Query>(1);
        let (follow_up_stream, follow_up_sink) = futures::sync::mpsc::channel::<pmodels::Query>(1);
        let query_stream = input_stream.select(follow_up_stream);

        let parser = (parser_builder)(protocol_mapping.clone());
        let dns_resolver = dns::Resolver::new(dns_resolver, dns_history);
        Self {
            parser,
            dns_resolver,

            socket_sink: Box::new(socket_sink),
            socket_stream: Box::new(socket_stream),

            input_sink,
            follow_up_sink,
            query_stream,

            protocol_mapping,
            dns_history,
        }
    }
}

#[async]
fn udp_send(
    socket: UdpSocket,
    dns_resolver: Arc<tokio_dns::Resolver>,
    addr: pmodels::Host,
    data: Vec<u8>,
) -> errors::Result<()> {
    if let Ok(addr) = await!(dns::resolve_host(dns_resolver.clone(), addr)) {
        let (socket, _) = await!(socket.send_dgram(data, addr))?;
    }

    Ok(socket)
}

impl Sink for UdpQuery {
    type SinkItem = pmodels::Query;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.query_sink.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        if self.socket_sink.as_mut() {
            /*
            SocketStatus::Sending((query, mut fut)) => {
                if let Async::Ready(socket) = fut.poll()? {
                    self.socket = SocketStatus::Ready(socket);
                    self.protocol_mapping
                        .lock()
                        .unwrap()
                        .insert(query.addr, query.protocol);
                }
                Ok(Async::NotReady)
            }

            SocketStatus::Ready(socket) => {
                if let Async::Ready(v) = self.query_stream.poll().unwrap() {
                    if let Some(query) = v {
                        let data = query.protocol.make_request();
                        self.socket = SocketStatus::Sending((
                            query,
                            Box::new(udp_send(
                                self.socket.clone(),
                                self.dns_resolver.clone(),
                                query.addr.clone(),
                                data,
                            )),
                        ));
                        Ok(Async::NotReady)
                    } else {
                        Ok(Async::Ready(()))
                    }
                } else {
                    Ok(Async::NotReady)
                }
            }
            */
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.query_sink.close()
    }
}

impl Stream for UdpQuery {
    type Item = ServerEntry;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Async::Ready(v) = self.parser.poll()? {
            match v {
                Some(data) => match data {
                    pmodels::ParseResult::FollowUp(s) => {
                        self.query_sink.try_send(s).unwrap();
                    }
                    pmodels::ParseResult::Output(s) => {
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

/// It can be used to spawn multiple UdpQueries.
pub struct UdpQueryServer {
    parser_builder: Box<Fn(ProtocolMapping) -> Box<Parser>>,
    dns_resolver: Box<Fn() -> Box<tokio_dns::Resolver>>,
}

impl UdpQueryServer {
    fn new() -> Self {
        Self {
            dns_resolver: Box::new(|| Box::new(tokio_dns::CpuPoolResolver::new(8))),
            parser_builder: Box::new(|| Box::new(RealParser::new())),
        }
    }

    fn with_dns_resolver<F: Fn() -> Box<tokio_dns::Resolver>>(self, f: F) -> Self {
        self.dns_resolver = Box::new(f);
        self
    }

    fn with_parser_builder<PB>(self, parser_builder: PB) -> Self
    where
        PB: Fn(ProtocolMapping) -> Box<Parser>,
    {
        self.parser_builder = parser_builder;
        self
    }

    fn into_query(&self, socket: UdpSocket) -> UdpQuery {
        UdpQuery::new(|s| (*self.parser_builder)(s), (self.dns_resolver)(), socket)
    }
}
