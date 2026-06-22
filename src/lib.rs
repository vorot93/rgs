//! Asynchronous utilities for querying Quake 3 game servers.

pub mod dns;
pub mod model;
pub mod q3;

use crate::{
    model::{Host, Server, ServerFilter},
    q3::Rule,
};
use bimap::BiMap;
use futures::Stream;
use hickory_resolver::TokioResolver;
use std::{
    borrow::Cow,
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, task::JoinHandle};
use tracing::debug;

/// A configured Quake 3 query engine.
#[derive(Clone)]
pub struct Client {
    resolver: TokioResolver,
    timeout: Duration,
    version: u32,
    rule_names: Cow<'static, BiMap<Rule, String>>,
    server_filter: ServerFilter,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Query a single server over one UDP socket.
    ///
    /// Returns `Ok(Some(server))` on success, `Ok(None)` if the server does not
    /// respond within the timeout or the filter drops it, and `Err` on a parse
    /// error. The round-trip time populates `Server::ping`.
    pub async fn query_server(
        &self,
        socket: Option<UdpSocket>,
        host: Host,
    ) -> anyhow::Result<Option<Server>> {
        let addr = dns::resolve(&self.resolver, host).await?;
        let socket = bind_or(socket).await?;

        let request = q3::make_getstatus();
        let sent = Instant::now();
        socket.send_to(&request, addr).await?;

        let mut buf = vec![0u8; 65_536];
        loop {
            match tokio::time::timeout(self.timeout, socket.recv_from(&mut buf)).await {
                Ok(Ok((n, src))) => {
                    if src != addr {
                        continue; // stray datagram from another peer
                    }
                    let rtt = sent.elapsed();
                    let parsed = q3::parse_status_response(
                        addr,
                        &buf[..n],
                        &self.rule_names,
                        &self.server_filter,
                    )?;
                    return Ok(parsed.map(|mut s| {
                        s.ping = Some(rtt);
                        s
                    }));
                }
                // An unconnected socket rarely surfaces ICMP errors; treat any
                // recv error as the host being unreachable.
                Ok(Err(e)) => {
                    debug!("recv error from {addr}: {e}");
                    return Ok(None);
                }
                // No response within the timeout: the server is down or silent.
                Err(_) => return Ok(None),
            }
        }
    }

    /// Query a master server over one UDP socket, optionally following up each
    /// listed server with a `getstatus`. Yields each server as its response
    /// arrives.
    ///
    /// With `follow_up == false`, yields a bare `Server::new(addr)` per listed
    /// address (no `getstatus` sent). With `follow_up == true`, yields fully
    /// parsed servers. A per-server parse error is yielded as an `Err` item; the
    /// run continues.
    pub fn query_master(
        &self,
        socket: Option<UdpSocket>,
        host: Host,
        follow_up: bool,
    ) -> impl Stream<Item = anyhow::Result<Server>> + 'static {
        let resolver = self.resolver.clone();
        let timeout = self.timeout;
        let version = self.version;
        let rule_names = self.rule_names.clone();
        let server_filter = self.server_filter.clone();

        async_stream::stream! {
            let master_addr = match dns::resolve(&resolver, host).await {
                Ok(a) => a,
                Err(e) => {
                    yield Err(e);
                    return;
                }
            };
            let socket = match bind_or(socket).await {
                Ok(s) => s,
                Err(e) => {
                    yield Err(e);
                    return;
                }
            };
            let socket = Arc::new(socket);

            // Dedicated receiver: stamp each datagram the instant it arrives and
            // forward it. Measuring RTT from this arrival time (rather than from
            // when the main loop next polls the socket) keeps the ping free of
            // parse, fan-out, and consumer-pacing delay. The channel is unbounded
            // so receiving never blocks on a slow consumer.
            let (tx, mut rx) =
                tokio::sync::mpsc::unbounded_channel::<(Vec<u8>, SocketAddr, Instant)>();
            let receiver = {
                let socket = socket.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65_536];
                    loop {
                        match socket.recv_from(&mut buf).await {
                            Ok((n, src)) => {
                                let recv_at = Instant::now();
                                if tx.send((buf[..n].to_vec(), src, recv_at)).is_err() {
                                    break; // the stream was dropped
                                }
                            }
                            // An unconnected socket rarely surfaces ICMP errors;
                            // a recv error ends reception (mirrors the old loop's
                            // break), and the main loop stops when the channel
                            // closes.
                            Err(e) => {
                                debug!("recv error: {e}");
                                break;
                            }
                        }
                    }
                })
            };
            // Abort the receiver when the stream ends or is dropped.
            let _receiver = AbortOnDrop(receiver);

            let request = q3::make_getservers(version);
            if let Err(e) = socket.send_to(&request, master_addr).await {
                yield Err(e.into());
                return;
            }

            let mut pending: HashMap<SocketAddr, Instant> = HashMap::new();
            let mut master_done = false;

            loop {
                if master_done && pending.is_empty() {
                    break;
                }

                let (data, src, recv_at) = match tokio::time::timeout(timeout, rx.recv()).await {
                    Ok(Some(item)) => item,
                    // Channel closed (the receiver hit a socket error and stopped)
                    // or the idle gap exceeded the timeout: stop reading.
                    Ok(None) | Err(_) => break,
                };

                if src == master_addr && !master_done {
                    match q3::parse_getservers_response(&data) {
                        Ok((addrs, eot)) => {
                            if eot {
                                master_done = true;
                            }
                            for v4 in addrs {
                                let sa = SocketAddr::V4(v4);
                                if follow_up {
                                    if pending.contains_key(&sa) {
                                        continue;
                                    }
                                    let getstatus = q3::make_getstatus();
                                    if let Err(e) = socket.send_to(&getstatus, sa).await {
                                        debug!("failed to send getstatus to {sa}: {e}");
                                        continue;
                                    }
                                    pending.insert(sa, Instant::now());
                                } else {
                                    yield Ok(Server::new(sa));
                                }
                            }
                        }
                        Err(e) => {
                            // The master's response is unparseable: give up on it
                            // but keep draining queried servers.
                            master_done = true;
                            yield Err(e);
                        }
                    }
                } else if let Some(sent) = pending.remove(&src) {
                    // recv_at was stamped on arrival, so this RTT excludes any
                    // parse, fan-out, or consumer-pacing delay.
                    let rtt = recv_at.saturating_duration_since(sent);
                    match q3::parse_status_response(src, &data, &rule_names, &server_filter) {
                        Ok(Some(mut server)) => {
                            server.ping = Some(rtt);
                            yield Ok(server);
                        }
                        Ok(None) => {}
                        Err(e) => yield Err(e),
                    }
                }
                // else: stray datagram from an unknown peer, ignore.
            }
        }
    }
}

/// Aborts a spawned task when dropped, so the master receiver task never
/// outlives the stream that owns it.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Use the consumer's socket, or bind a default IPv4 socket.
///
/// IPv4 is deliberate: every q3 destination (the master and every server it
/// lists) is IPv4, so an IPv4 socket reaches all of them and avoids the macOS
/// "IPv4 destination from an IPv6 socket" `EINVAL` failure (see commit 18eeaa1).
/// Never bind `[::]`; never v4-map destinations.
async fn bind_or(socket: Option<UdpSocket>) -> anyhow::Result<UdpSocket> {
    match socket {
        Some(s) => Ok(s),
        None => {
            let bind: SocketAddr = (Ipv4Addr::UNSPECIFIED, 0).into();
            Ok(UdpSocket::bind(bind).await?)
        }
    }
}

/// Builder for [`Client`].
pub struct ClientBuilder {
    resolver: TokioResolver,
    timeout: Duration,
    version: u32,
    rule_names: Cow<'static, BiMap<Rule, String>>,
    server_filter: ServerFilter,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            resolver: TokioResolver::builder_tokio().unwrap().build().unwrap(),
            timeout: Duration::from_secs(5),
            version: 68,
            rule_names: Cow::Borrowed(q3::default_rule_names()),
            server_filter: ServerFilter::default(),
        }
    }
}

impl ClientBuilder {
    pub fn resolver(mut self, resolver: TokioResolver) -> Self {
        self.resolver = resolver;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    pub fn rule_names(mut self, rule_names: impl Into<BiMap<Rule, String>>) -> Self {
        self.rule_names = Cow::Owned(rule_names.into());
        self
    }

    pub fn server_filter(mut self, server_filter: ServerFilter) -> Self {
        self.server_filter = server_filter;
        self
    }

    pub fn build(self) -> Client {
        Client {
            resolver: self.resolver,
            timeout: self.timeout,
            version: self.version,
            rule_names: self.rule_names,
            server_filter: self.server_filter,
        }
    }
}
