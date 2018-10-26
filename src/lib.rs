//!
//! Asynchronous utilities for querying game servers.
//!
//! The `rgs` crate provides tools to asynchronously retrieve game server information like
//! IP, server metadata, player list and more.

use failure::{format_err, Fail, Fallible};
use futures::{
    empty,
    future::ok,
    prelude::*,
    stream::{futures_unordered, FuturesUnordered},
    sync::mpsc::UnboundedSender,
};
use log::{debug, trace};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::{UdpFramed, UdpSocket};
use tokio_codec::BytesCodec;

pub mod dns;
#[macro_use]
pub mod errors;
pub mod models;
pub use self::models::*;
pub mod ping;
pub mod protocols;

use self::dns::Resolver;
use self::ping::Pinger;

type ProtocolMapping = Arc<Mutex<HashMap<SocketAddr, TProtocol>>>;

fn to_v4(addr: SocketAddr) -> SocketAddr {
    use self::SocketAddr::*;

    if let V6(v) = addr {
        if let Some(v4_addr) = v.ip().to_ipv4() {
            return SocketAddr::from((v4_addr, v.port()));
        }
    }

    addr
}

pub enum FullParseResult {
    FollowUp(Query),
    Output(ServerEntry),
    Error((Option<Packet>, failure::Error)),
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
    type SinkItem = Fallible<IncomingPacket>;
    type SinkError = failure::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let mut results_stream = self.results_stream.take().unwrap();
        match item {
            Ok(incoming_packet) => {
                let (protocol, pkt, ping) = incoming_packet.into();

                results_stream = Box::new(
                    results_stream.chain(
                        Protocol::parse_response(&**protocol, pkt.clone())
                            .map(move |v| match v {
                                ParseResult::FollowUp(q) => FullParseResult::FollowUp(
                                    (q, TProtocol::clone(&protocol)).into(),
                                ),
                                ParseResult::Output(s) => FullParseResult::Output(ServerEntry {
                                    protocol: protocol.clone(),
                                    data: Server {
                                        ping: Some(ping),
                                        ..s
                                    },
                                }),
                            })
                            .or_else(move |(pkt, e)| Ok(FullParseResult::Error((pkt, e)))),
                    ),
                );
            }
            Err(e) => {
                results_stream = Box::new(results_stream.chain(futures::stream::iter_ok(vec![
                    FullParseResult::Error((None, e)),
                ])))
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
    pub ping: Duration,
    pub data: Vec<u8>,
}

impl From<IncomingPacket> for (TProtocol, Packet, Duration) {
    fn from(v: IncomingPacket) -> Self {
        (
            v.protocol,
            Packet {
                addr: v.addr,
                data: v.data,
            },
            v.ping,
        )
    }
}

pub type PingerFuture = Box<Future<Item = ServerEntry, Error = failure::Error> + Send>;

/// Represents a single request by user to query the servers fed into sink.
pub struct UdpQuery {
    query_to_dns: Box<Future<Item = (), Error = failure::Error> + Send>,
    dns_to_socket: Box<Future<Item = (), Error = failure::Error> + Send>,
    socket_to_parser: Box<Future<Item = (), Error = failure::Error> + Send>,

    parser_stream: Box<Stream<Item = FullParseResult, Error = failure::Error> + Send>,
    input_sink: UnboundedSender<Query>,
    follow_up_sink: UnboundedSender<Query>,

    pinger: Arc<Pinger>,
    pinger_cache: Arc<Mutex<HashMap<IpAddr, Duration>>>,
    pinger_stream: FuturesUnordered<PingerFuture>,
}

impl UdpQuery {
    fn new<D, P>(dns_resolver: D, pinger: P, socket: UdpSocket) -> Self
    where
        D: Into<Arc<Resolver>>,
        P: Into<Arc<Pinger>>,
    {
        let ping_mapping = Arc::new(Mutex::new(HashMap::<SocketAddr, Instant>::new()));
        let protocol_mapping = ProtocolMapping::default();
        let dns_history = dns::History::default();

        let (socket_sink, socket_stream) = UdpFramed::new(socket, BytesCodec::new()).split();

        let socket_stream = socket_stream
            .map({
                let ping_mapping = ping_mapping.clone();
                let protocol_mapping = protocol_mapping.clone();
                move |(buf, mut addr)| {
                    let now = Instant::now();

                    addr = to_v4(addr);

                    let ping = now.duration_since(
                        ping_mapping
                            .lock()
                            .unwrap()
                            .get(&addr)
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!("Could not determine ping for {}", addr),
                                )
                            })?
                            .clone(),
                    );
                    trace!("Received data from {}: {:?}", addr, buf);
                    let protocol = protocol_mapping
                        .lock()
                        .unwrap()
                        .get(&addr)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("Could not determine protocol for {}", addr),
                            )
                        })?
                        .clone();
                    Ok(IncomingPacket {
                        protocol,
                        addr,
                        ping,
                        data: buf.to_vec(),
                    })
                }
            })
            .map_err(failure::Error::from);

        let socket_sink =
            socket_sink.sink_map_err(|e| failure::Error::from(e.context("Socket sink error")));
        let socket_stream =
            socket_stream.map_err(|e| failure::Error::from(e.context("Socket stream error")));

        let (query_sink, query_stream) = futures::sync::mpsc::unbounded::<Query>();
        let query_stream =
            Box::new(query_stream.map_err(|_| format_err!("Query stream returned an error")));

        let (input_sink, follow_up_sink) = (query_sink.clone(), query_sink.clone());

        let parser = ParseMuxer::new();
        let dns_resolver = dns::ResolverPipe::new(dns_resolver.into(), dns_history.clone());

        let (parser_sink, parser_stream) = parser.split();
        let parser_stream = Box::new(
            parser_stream.map_err(|e| failure::Error::from(e.context("Parser stream error"))),
        );
        let (dns_resolver_sink, dns_resolver_stream) = dns_resolver.split();

        let dns_resolver_stream = dns_resolver_stream.map({
            let ping_mapping = ping_mapping.clone();
            let protocol_mapping = protocol_mapping.clone();
            move |v| {
                let data = v.protocol.make_request(v.state);
                trace!("Sending data to {}: {:?}", v.addr, data);
                protocol_mapping.lock().unwrap().insert(v.addr, v.protocol);
                ping_mapping.lock().unwrap().insert(v.addr, Instant::now());
                (data.into(), v.addr)
            }
        });

        // Outgoing pipe: Query Stream -> DNS Resolver -> UDP Socket
        let query_to_dns = Box::new(dns_resolver_sink.send_all(query_stream).map(|_| ()));
        let dns_to_socket = Box::new(socket_sink.send_all(dns_resolver_stream).map(|_| ()));

        // Incoming pipe: UDP Socket -> Parser
        let socket_to_parser = Box::new(parser_sink.send_all(socket_stream).map(|_| ()));

        let pinger_cache = Default::default();
        let pinger_stream = futures_unordered(vec![Box::new(empty()) as PingerFuture]);

        Self {
            query_to_dns,
            dns_to_socket,
            socket_to_parser,

            parser_stream,
            input_sink,
            follow_up_sink,

            pinger: pinger.into(),
            pinger_cache,
            pinger_stream,
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
        debug!("Polled UdpQuery");

        self.query_to_dns.poll()?;
        self.dns_to_socket.poll()?;
        self.socket_to_parser.poll()?;

        if self.pinger_stream.len() < 20 {
            if let Async::Ready(v) = self.parser_stream.poll()? {
                match v {
                    Some(data) => match data {
                        FullParseResult::FollowUp(s) => {
                            self.follow_up_sink.unbounded_send(s).unwrap();
                        }
                        FullParseResult::Output(mut srv) => {
                            let addr = srv.data.addr;
                            self.pinger_stream.push(
                                if let Some(cached_ping) =
                                    self.pinger_cache.lock().unwrap().get(&addr.ip())
                                {
                                    trace!("Found cached ping data for {}", addr);
                                    srv.data.ping = Some(*cached_ping);
                                    Box::new(ok(srv))
                                } else {
                                    Box::new(self.pinger.ping(addr.ip()).then({
                                        let pinger_cache = self.pinger_cache.clone();
                                        move |rtt| {
                                            match rtt {
                                                Ok(v) => {
                                                    if let Some(v) = v {
                                                        pinger_cache
                                                            .lock()
                                                            .unwrap()
                                                            .insert(addr.ip(), v);

                                                        srv.data.ping = Some(v);
                                                    }
                                                }
                                                Err(e) => {
                                                    debug!("Failed to ping {}: {}", addr, e);
                                                }
                                            }
                                            Ok(srv)
                                        }
                                    }))
                                },
                            );
                        }
                        FullParseResult::Error(e) => {
                            debug!(
                                "Parser returned error. Addr: {:?}, Data: {:?}, Error: {:?}",
                                e.0.clone().map(|e| e.addr),
                                e.0.clone()
                                    .map(|e| String::from_utf8_lossy(&e.data).to_string()),
                                e.1
                            );
                        }
                    },
                    None => {
                        return Ok(Async::NotReady);
                    }
                }
            }
        }

        if let Async::Ready(srv) = self.pinger_stream.poll()? {
            return Ok(Async::Ready(srv));
        }

        Ok(Async::NotReady)
    }
}

/// It can be used to spawn multiple UdpQueries.
pub struct UdpQueryBuilder {
    pinger: Arc<Pinger>,
    dns_resolver: Arc<Resolver>,
}

impl Default for UdpQueryBuilder {
    fn default() -> Self {
        Self {
            pinger: Arc::new(ping::DummyPinger),
            dns_resolver: Arc::new(tokio_dns::CpuPoolResolver::new(8)) as Arc<Resolver>,
        }
    }
}

impl UdpQueryBuilder {
    pub fn with_dns_resolver(mut self, resolver: Arc<Resolver>) -> Self {
        self.dns_resolver = resolver;
        self
    }

    pub fn with_pinger<T>(mut self, pinger: T) -> Self
    where
        T: Into<Arc<Pinger>>,
    {
        self.pinger = pinger.into();
        self
    }

    pub fn build(&self, socket: UdpSocket) -> UdpQuery {
        UdpQuery::new(self.dns_resolver.clone(), self.pinger.clone(), socket)
    }
}

pub fn simple_udp_query(input: Vec<UserQuery>) -> UdpQuery {
    let query_builder = UdpQueryBuilder::default();

    let socket = UdpSocket::bind(&"[::]:5678".parse().unwrap()).unwrap();
    let mut q = query_builder.build(socket);

    for entry in input {
        q.start_send(entry).unwrap();
    }

    q
}
