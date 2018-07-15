//!
//! Asynchronous utilities for querying game servers.
//!
//! The `rgs` crate provides tools to asynchronously retrieve game server information like
//! IP, server metadata, player list and more.

extern crate byteorder;
extern crate chrono;
#[macro_use]
extern crate failure;
extern crate futures;
extern crate handlebars;
extern crate iso_country;
extern crate log;
#[macro_use]
extern crate maplit;
extern crate num_traits;
extern crate openttd;
extern crate q3a;
extern crate rand;
extern crate resolve;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[cfg_attr(test, macro_use)]
extern crate serde_json;
extern crate tokio;
extern crate tokio_codec;
extern crate tokio_dns;
extern crate tokio_io;

use failure::Fail;
use futures::prelude::*;
use futures::sync::mpsc::Sender;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::{UdpFramed, UdpSocket};
use tokio_codec::BytesCodec;

pub mod dns;
#[macro_use]
pub mod errors;
pub mod models;
pub use models::*;
pub mod protocols;
#[macro_use]
pub mod util;

type ProtocolMapping = Arc<Mutex<HashMap<SocketAddr, TProtocol>>>;

pub enum FullParseResult {
    FollowUp(Query),
    Output(ServerEntry),
    Error(failure::Error),
}

struct ParseMuxer {
    results_stream: Option<Box<Stream<Item = FullParseResult, Error = failure::Error> + Send>>,
}

impl ParseMuxer {
    pub fn new() -> Self {
        Self {
            results_stream: Some(Box::new(futures::stream::iter_ok(vec![]))),
        }
    }
}

impl Sink for ParseMuxer {
    type SinkItem = Result<IncomingPacket, failure::Error>;
    type SinkError = failure::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let mut results_stream = self.results_stream.take().unwrap();
        match item {
            Ok(incoming_packet) => {
                let (protocol, pkt) = incoming_packet.into();

                results_stream = Box::new(
                    results_stream.chain(
                        Protocol::parse_response(&**protocol, pkt)
                            .map(move |v| match v {
                                ParseResult::FollowUp(q) => FullParseResult::FollowUp(
                                    (q, TProtocol::clone(&protocol)).into(),
                                ),
                                ParseResult::Output(s) => FullParseResult::Output(ServerEntry {
                                    protocol: protocol.clone(),
                                    data: s,
                                }),
                            })
                            .or_else(|e| Ok(FullParseResult::Error(e))),
                    ),
                );
            }
            Err(e) => {
                results_stream = Box::new(
                    results_stream.chain(futures::stream::iter_ok(vec![FullParseResult::Error(e)])),
                )
            }
        }
        self.results_stream = Some(results_stream);

        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.poll_complete()
    }
}

impl Stream for ParseMuxer {
    type Item = FullParseResult;
    type Error = failure::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.results_stream.as_mut().unwrap().poll()
    }
}

struct IncomingPacket {
    pub addr: SocketAddr,
    pub protocol: TProtocol,
    pub data: Vec<u8>,
}

impl From<IncomingPacket> for (TProtocol, Packet) {
    fn from(v: IncomingPacket) -> Self {
        (
            v.protocol,
            Packet {
                addr: v.addr,
                data: v.data,
            },
        )
    }
}

/// Represents a single request by user to query the servers fed into sink.
pub struct UdpQuery {
    query_to_dns: Box<Future<Item = (), Error = failure::Error> + Send>,
    dns_to_socket: Box<Future<Item = (), Error = failure::Error> + Send>,
    socket_to_parser: Box<Future<Item = (), Error = failure::Error> + Send>,

    parser_stream: Box<Stream<Item = FullParseResult, Error = failure::Error> + Send>,
    input_sink: Sender<Query>,
    follow_up_sink: Sender<Query>,
}

impl UdpQuery {
    fn new(
        dns_resolver: Arc<tokio_dns::Resolver + Send + Sync + 'static>,
        socket: UdpSocket,
    ) -> Self {
        let protocol_mapping = ProtocolMapping::default();
        let dns_history = dns::History::default();

        let (socket_sink, socket_stream) = UdpFramed::new(socket, BytesCodec::new()).split();

        let socket_stream = socket_stream
            .map({
                let protocol_mapping = protocol_mapping.clone();
                move |(buf, addr)| {
                    let protocol = protocol_mapping
                        .lock()
                        .unwrap()
                        .get(&addr)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Could not determine server's protocol",
                            )
                        })?
                        .clone();
                    Ok(IncomingPacket {
                        protocol,
                        addr: addr,
                        data: buf.to_vec(),
                    })
                }
            })
            .map_err(|e| failure::Error::from(e));

        let socket_sink =
            socket_sink.sink_map_err(|e| failure::Error::from(e.context("Socket sink error")));
        let socket_stream =
            socket_stream.map_err(|e| failure::Error::from(e.context("Socket stream error")));

        let (query_sink, query_stream) = futures::sync::mpsc::channel::<Query>(1);
        let query_stream =
            Box::new(query_stream.map_err(|_| format_err!("Query stream returned an error")));

        let (input_sink, follow_up_sink) = (query_sink.clone(), query_sink.clone());

        let parser = ParseMuxer::new();
        let dns_resolver = dns::Resolver::new(dns_resolver, dns_history.clone());

        let (parser_sink, parser_stream) = parser.split();
        let parser_stream = Box::new(
            parser_stream.map_err(|e| failure::Error::from(e.context("Parser stream error"))),
        );
        let (dns_resolver_sink, dns_resolver_stream) = dns_resolver.split();

        let dns_resolver_stream = dns_resolver_stream.map({
            let protocol_mapping = protocol_mapping.clone();
            move |v| {
                let data = v.protocol.make_request(v.state);
                protocol_mapping
                    .lock()
                    .unwrap()
                    .insert(v.addr.clone(), v.protocol);
                (data.into(), v.addr)
            }
        });

        // Outgoing pipe: Query Stream -> DNS Resolver -> UDP Socket
        let query_to_dns = Box::new(dns_resolver_sink.send_all(query_stream).map(|_| ()));
        let dns_to_socket = Box::new(socket_sink.send_all(dns_resolver_stream).map(|_| ()));

        // Incoming pipe: UDP Socket -> Parser
        let socket_to_parser = Box::new(parser_sink.send_all(socket_stream).map(|_| ()));

        Self {
            query_to_dns,
            dns_to_socket,
            socket_to_parser,

            parser_stream,
            input_sink,
            follow_up_sink,
        }
    }
}

impl Sink for UdpQuery {
    type SinkItem = UserQuery;
    type SinkError = failure::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        Ok(self.input_sink.start_send(item.into())?.map(|v| v.into()))
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.input_sink.poll_complete()?)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.input_sink.close()?)
    }
}

impl Stream for UdpQuery {
    type Item = ServerEntry;
    type Error = failure::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.query_to_dns.poll()?;
        self.dns_to_socket.poll()?;
        self.socket_to_parser.poll()?;

        if let Async::Ready(v) = self.parser_stream.poll()? {
            match v {
                Some(data) => match data {
                    FullParseResult::FollowUp(s) => {
                        self.follow_up_sink.try_send(s).unwrap();
                    }
                    FullParseResult::Output(s) => {
                        return Ok(Async::Ready(Some(s)));
                    }
                    FullParseResult::Error(e) => {
                        eprintln!("Parser returned error: {:?}", e);
                    }
                },
                None => {
                    return Ok(Async::NotReady);
                }
            }
        }

        Ok(Async::NotReady)
    }
}

/// It can be used to spawn multiple UdpQueries.
pub struct UdpQueryBuilder {
    dns_resolver: Arc<tokio_dns::Resolver + Send + Sync + 'static>,
}

impl Default for UdpQueryBuilder {
    fn default() -> Self {
        Self {
            dns_resolver: Arc::new(tokio_dns::CpuPoolResolver::new(8))
                as Arc<tokio_dns::Resolver + Send + Sync + 'static>,
        }
    }
}

impl UdpQueryBuilder {
    pub fn with_dns_resolver(
        mut self,
        resolver: Arc<tokio_dns::Resolver + Send + Sync + 'static>,
    ) -> Self {
        self.dns_resolver = resolver;
        self
    }

    pub fn build(&self, socket: UdpSocket) -> UdpQuery {
        UdpQuery::new(Arc::clone(&self.dns_resolver), socket)
    }
}
