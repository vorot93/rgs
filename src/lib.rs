//!
//! Asynchronous utilities for querying game servers.
//!
//! The `rgs` crate provides tools to asynchronously retrieve game server information like
//! IP, server metadata, player list and more.

pub mod dns;
pub mod errors;
pub mod models;
pub mod ping;
pub mod protocols;

use crate::{dns::Resolver, models::*, ping::Pinger};
use bytes::Bytes;
use futures::{
    future::{ok, BoxFuture},
    prelude::*,
    stream::{BoxStream, FuturesUnordered},
};
use std::{
    collections::HashMap,
    future::pending,
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, sync::mpsc::UnboundedSender};
use tokio_stream::once;
use tokio_util::{codec::BytesCodec, udp::UdpFramed};
use tracing::*;

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
    Error((Option<Packet>, anyhow::Error)),
}

struct ParseMuxer {
    results_stream: Option<BoxStream<'static, FullParseResult>>,
}

impl ParseMuxer {
    pub fn new() -> Self {
        Self {
            results_stream: Some(Box::pin(futures::stream::empty())),
        }
    }
}

impl Sink<anyhow::Result<IncomingPacket>> for ParseMuxer {
    type Error = anyhow::Error;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: anyhow::Result<IncomingPacket>,
    ) -> Result<(), Self::Error> {
        let mut results_stream = self.results_stream.take().unwrap();
        match item {
            Ok(incoming_packet) => {
                let (protocol, pkt, ping) = incoming_packet.into();

                results_stream = Box::pin(results_stream.chain(
                    Protocol::parse_response(&**protocol, pkt).map(move |v| {
                        v.map(|v| match v {
                            ParseResult::FollowUp(q) => {
                                FullParseResult::FollowUp((q, TProtocol::clone(&protocol)).into())
                            }
                            ParseResult::Output(s) => FullParseResult::Output(ServerEntry {
                                protocol: protocol.clone(),
                                data: Server {
                                    ping: Some(ping),
                                    ..s
                                },
                            }),
                        })
                        .unwrap_or_else(move |(pkt, e)| FullParseResult::Error((pkt, e)))
                    }),
                ));
            }
            Err(e) => {
                results_stream =
                    Box::pin(results_stream.chain(once(FullParseResult::Error((None, e)))));
            }
        }
        self.results_stream = Some(results_stream);

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Stream for ParseMuxer {
    type Item = FullParseResult;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.results_stream.as_mut().unwrap().as_mut().poll_next(cx)
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

pub type PingerFuture = BoxFuture<'static, anyhow::Result<ServerEntry>>;

/// Represents a single request by user to query the servers fed into sink.
pub struct UdpQuery {
    query_to_dns: BoxFuture<'static, anyhow::Result<()>>,
    dns_to_socket: BoxFuture<'static, anyhow::Result<()>>,
    socket_to_parser: BoxFuture<'static, anyhow::Result<()>>,

    parser_stream: BoxStream<'static, FullParseResult>,
    input_sink: UnboundedSender<Query>,
    follow_up_sink: UnboundedSender<Query>,

    pinger: Arc<dyn Pinger>,
    pinger_cache: Arc<Mutex<HashMap<IpAddr, Duration>>>,
    pinger_stream: FuturesUnordered<PingerFuture>,
}

impl UdpQuery {
    fn new<D, P>(dns_resolver: D, pinger: P, socket: UdpSocket) -> Self
    where
        D: Into<Arc<dyn Resolver>>,
        P: Into<Arc<dyn Pinger>>,
    {
        let ping_mapping = Arc::new(Mutex::new(HashMap::new()));
        let protocol_mapping = ProtocolMapping::default();
        let dns_history = dns::History::default();

        let (socket_sink, socket_stream) =
            UdpFramed::new(socket, BytesCodec::new()).split::<(Bytes, _)>();

        let socket_stream = socket_stream
            .map_ok({
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
                            .copied()
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!("Could not determine ping for {}", addr),
                                )
                            })?,
                    );
                    trace!("Received data from {}: {}", addr, hex::encode(&buf));
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
            .map_err(anyhow::Error::from);

        let socket_sink =
            socket_sink.sink_map_err(|e| anyhow::Error::from(e).context("Socket sink error"));
        let socket_stream = socket_stream.map_err(|e| e.context("Socket stream error"));

        let (query_sink, query_stream) = tokio::sync::mpsc::unbounded_channel::<Query>();
        let query_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(query_stream);

        let (input_sink, follow_up_sink) = (query_sink.clone(), query_sink);

        let parser = ParseMuxer::new();
        let dns_resolver = dns::ResolverPipe::new(dns_resolver.into(), dns_history);

        let (parser_sink, parser_stream) = parser.split();
        let parser_stream = Box::pin(parser_stream);
        let (dns_resolver_sink, dns_resolver_stream) = dns_resolver.split();

        let dns_resolver_stream = dns_resolver_stream.map({
            move |v| {
                let data = v.protocol.make_request(v.state);
                trace!("Sending data to {}: {}", v.addr, hex::encode(&data));
                protocol_mapping.lock().unwrap().insert(v.addr, v.protocol);
                ping_mapping.lock().unwrap().insert(v.addr, Instant::now());
                (data.into(), v.addr)
            }
        });

        // Outgoing pipe: Query Stream -> DNS Resolver -> UDP Socket
        let query_to_dns = Box::pin(
            query_stream
                .map(Ok::<_, anyhow::Error>)
                .forward(dns_resolver_sink),
        );
        let dns_to_socket = Box::pin(
            dns_resolver_stream
                .map(Ok::<_, anyhow::Error>)
                .forward(socket_sink),
        );

        // Incoming pipe: UDP Socket -> Parser
        let socket_to_parser = Box::pin(socket_stream.forward(parser_sink));

        let pinger_cache = Default::default();
        let pinger_stream = vec![Box::pin(pending()) as PingerFuture]
            .into_iter()
            .collect::<FuturesUnordered<_>>();

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

    pub async fn simple_query(input: Vec<UserQuery>) -> Self {
        let query_builder = UdpQueryBuilder::default();

        let socket = UdpSocket::bind(&"[::]:5678").await.unwrap();
        let q = query_builder.build(socket);

        for entry in input {
            q.input_sink.send(entry.into()).unwrap();
        }

        q
    }

    pub fn queue(&self, query: UserQuery) -> bool {
        self.input_sink.send(query.into()).is_ok()
    }
}

impl Stream for UdpQuery {
    type Item = anyhow::Result<ServerEntry>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        debug!("Polled UdpQuery");

        let _ = Pin::new(&mut self.query_to_dns).poll(cx)?;
        let _ = Pin::new(&mut self.dns_to_socket).poll(cx)?;
        let _ = Pin::new(&mut self.socket_to_parser).poll(cx)?;

        if self.pinger_stream.len() < 20 {
            if let Poll::Ready(v) = self.parser_stream.as_mut().poll_next(cx) {
                match v {
                    Some(data) => match data {
                        FullParseResult::FollowUp(s) => {
                            self.follow_up_sink.send(s).unwrap();
                        }
                        FullParseResult::Output(mut srv) => {
                            let addr = srv.data.addr;
                            self.pinger_stream.push(
                                if let Some(cached_ping) =
                                    self.pinger_cache.lock().unwrap().get(&addr.ip())
                                {
                                    trace!("Found cached ping data for {}", addr);
                                    srv.data.ping = Some(*cached_ping);
                                    Box::pin(ok(srv))
                                } else {
                                    Box::pin(self.pinger.ping(addr.ip()).then({
                                        let pinger_cache = self.pinger_cache.clone();
                                        move |rtt| async move {
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
                                e.0.as_ref().map(|e| e.addr),
                                e.0.map(|e| hex::encode(e.data)),
                                e.1
                            );
                        }
                    },
                    None => {
                        return Poll::Pending;
                    }
                }
            }
        }

        Pin::new(&mut self.pinger_stream).poll_next(cx)
    }
}

/// It can be used to spawn multiple UdpQueries.
pub struct UdpQueryBuilder {
    pinger: Arc<dyn Pinger>,
    dns_resolver: Arc<dyn Resolver>,
}

impl Default for UdpQueryBuilder {
    fn default() -> Self {
        Self {
            pinger: Arc::new(()),
            dns_resolver: Arc::new(
                trust_dns_resolver::TokioAsyncResolver::tokio_from_system_conf().unwrap(),
            ) as Arc<dyn Resolver>,
        }
    }
}

impl UdpQueryBuilder {
    pub fn with_dns_resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.dns_resolver = resolver;
        self
    }

    pub fn with_pinger<T>(mut self, pinger: T) -> Self
    where
        T: Into<Arc<dyn Pinger>>,
    {
        self.pinger = pinger.into();
        self
    }

    pub fn build(&self, socket: UdpSocket) -> UdpQuery {
        UdpQuery::new(self.dns_resolver.clone(), self.pinger.clone(), socket)
    }
}
