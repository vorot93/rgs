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

pub struct RealParser {
    packet_sink: Sender<pmodels::Packet>,
    packet_stream: Receiver<pmodels::Packet>,
    results_stream: Box<Stream<Item = pmodels::ParseResult, Error = Error>>,
}

impl RealParser {
    pub fn new() -> Self {
        let (data_in, data_out) = futures::sync::mpsc::channel(1);
        Self {
            packet_sink: data_in.sink_map_err(|_| Error::NetworkError { what: "".into() }),
            packet_stream: data_out.map_err(|_| Error::NetworkError { what: "".into() }),
            results_stream: Box::new(futures::stream::iter_ok(vec![])),
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
            *self.results_stream = self.results_stream
                .chain(pmodels::Protocol::parse_response(&*protocol, &pkt));
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
    type Item = pmodels::ParseResult;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.results_stream.poll()
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

enum SocketStatus {
    Ready(UdpSocket),
    Sending(impl Future<Item = UdpSocket, Error = Error>),
}

pub struct UdpQuery {
    socket: SocketStatus,
    dns_resolver: Arc<tokio_dns::Resolver>,
    outgoing_send: Option<Box<Future<Item = (), Error = Error>>>,

    parser_stream: Arc<Stream<Item = ParseResult, Error = errors::Error>>,
    query_sink: Sender<pmodels::Query>,
    query_stream: Receiver<pmodels::Query>,
    packet_sink: Sender<pmodels::Packet>,
    packet_stream: Receiver<pmodels::Packet>,
}

impl UdpQuery {
    fn new<
        P: Stream<Item = pmodels::ParseResult, Error = Error>,
        PF: FnOnce(Q) -> Box<P>,
        D: tokio_dns::Resolver + 'static,
    >(
        parser_builder: PF,
        user_input: Q,
        dns_resolver: Arc<D>,
        socket: UdpSocket,
    ) -> Self {
        let (packet_stream, packet_sink) = futures::sync::mpsc::channel::<pmodels::Packet>(1);
        let (query_stream, query_sink) = futures::sync::mpsc::channel::<pmodels::Query>(1);
        Self {
            parser_stream: Arc::new((parser_builder)(rcv_queue_out)),
            query_stream: Arc::new(query_stream),
            socket: Arc::new(socket),
            query_sink,
            dns_resolver,
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
    if let Ok(addr) = await!(dns_resolve(dns_resolver.clone(), addr)) {
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
        match self.socket.as_mut() {
            SocketStatus::Sending(fut) => {
                if Async::Ready(socket) = fut.poll()? {
                    self.socket = SocketStatus::Ready(socket);
                }
                Ok(Async::NotReady)
            }

            SocketStatus::Ready(socket) => {
                if let Async::Ready(v) = self.query_stream.poll().unwrap() {
                    if let Some(query) = v {
                        let data = query.protocol.make_request();
                        self.socket = SocketStatus::Sending(udp_send(
                            self.socket.clone(),
                            self.dns_resolver.clone(),
                            query.addr,
                            data,
                        ));
                        Ok(Async::NotReady)
                    } else {
                        Ok(Async::Ready(()))
                    }
                } else {
                    Ok(Async::NotReady)
                }
            }
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        async_block! {
            await!(self.send_queue_in.close())?;

            Ok(())
        }
    }
}

impl Stream for UdpQuery {
    type Item = ServerEntry;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.rcv_queue_in.poll_ready()? {
            let mut data = vec![];
            if let Async::Ready(_) = self.socket.recv_dgram(&mut data) {
                self.rcv_queue_in.try_send(pmodels::Packet { addr, data })
            }
        }

        if let Async::Ready(v) = self.parser_stream.poll()? {
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

pub struct UdpQueryServer {
    parser_builder: Box<
        Fn(
            
        ) -> Box<
            Sink<SinkItem = Packet, SinkError = Error> + Stream<Item = ParseResult, Error = Error>,
        >,
    >,
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
        PB: Fn(
            
        ) -> Box<
            Sink<SinkItem = Packet, SinkError = Error> + Stream<Item = ParseResult, Error = Error>,
        >,
    {
        self.parser_builder = parser_builder;
        self
    }

    fn into_query(&self, socket: UdpSocket) -> UdpQuery {
        UdpQuery::new(|s| (*self.parser_builder)(s), (self.dns_resolver)(), socket)
    }
}
