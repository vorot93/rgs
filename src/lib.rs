//! Asynchronous utilities for querying game servers.
//!
//! The `rgs` crate provides tools to asynchronously retrieve game server
//! information like IP, server metadata, player list and more.

pub mod dns;
pub mod model;
pub mod ping;
pub mod protocol;

use crate::{
    dns::Resolver,
    model::{Query, ServerEntry},
    ping::Pinger,
    protocol::Outcome,
};
use futures::{Stream, StreamExt, stream::FuturesUnordered};
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, sync::Semaphore};
use tracing::{debug, trace};

/// A configured query engine.
#[derive(Clone)]
pub struct Client {
    resolver: Arc<dyn Resolver>,
    pinger: Arc<dyn Pinger>,
    concurrency: usize,
    timeout: Duration,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Run all `queries`, fanning out follow-ups, and yield each server as it is
    /// found. The stream drains and ends once no work remains.
    pub fn query(
        &self,
        queries: impl IntoIterator<Item = Query>,
    ) -> impl Stream<Item = anyhow::Result<ServerEntry>> {
        let resolver = self.resolver.clone();
        let pinger = self.pinger.clone();
        let timeout = self.timeout;
        let sem = Arc::new(Semaphore::new(self.concurrency));
        let initial: Vec<Query> = queries.into_iter().collect();

        async_stream::stream! {
            let mut inflight = FuturesUnordered::new();
            for q in initial {
                inflight.push(run_query(q, resolver.clone(), pinger.clone(), sem.clone(), timeout));
            }

            while let Some(res) = inflight.next().await {
                match res {
                    Ok(run) => {
                        for f in run.follow_ups {
                            inflight.push(run_query(
                                f,
                                resolver.clone(),
                                pinger.clone(),
                                sem.clone(),
                                timeout,
                            ));
                        }
                        for entry in run.outputs {
                            yield Ok(entry);
                        }
                    }
                    Err(e) => yield Err(e),
                }
            }
        }
    }
}

/// The result of one `run_query`: emitted servers plus follow-up queries to run.
struct Run {
    outputs: Vec<ServerEntry>,
    follow_ups: Vec<Query>,
}

async fn run_query(
    query: Query,
    resolver: Arc<dyn Resolver>,
    pinger: Arc<dyn Pinger>,
    sem: Arc<Semaphore>,
    timeout: Duration,
) -> anyhow::Result<Run> {
    // Held for this query's DNS + I/O; released when the function returns, before
    // the caller pushes any follow-ups.
    let _permit = sem.acquire().await.expect("semaphore is never closed");

    let addr = resolver.resolve(query.host.clone()).await?;

    // Bind a socket of the same address family as the target, so an IPv4 server
    // is reached from an IPv4 socket (no v4-mapped-IPv6 workaround).
    let bind: SocketAddr = if addr.is_ipv6() {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(bind).await?;
    socket.connect(addr).await?;

    let request = query.protocol.make_request();
    let sent = Instant::now();
    socket.send(&request).await?;

    let mut outcomes = Vec::new();
    let mut buf = vec![0u8; 65_536];
    loop {
        let n = match tokio::time::timeout(timeout, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => n,
            // Down server / ICMP port-unreachable on a connected socket: silently stop.
            Ok(Err(e)) => {
                debug!("recv error from {addr}: {e}");
                break;
            }
            // No response within the timeout: silently stop.
            Err(_) => {
                trace!("query to {addr} timed out");
                break;
            }
        };

        let parsed = query.protocol.parse_response(addr, &buf[..n])?;
        outcomes.extend(parsed.outcomes);
        if !parsed.expect_more {
            break;
        }
    }

    let rtt = sent.elapsed();
    let mut outputs = Vec::new();
    let mut follow_ups = Vec::new();
    for outcome in outcomes {
        match outcome {
            Outcome::Server(mut server) => {
                server.ping = Some(rtt);
                if let Ok(Some(icmp)) = pinger.ping(addr.ip()).await {
                    server.ping = Some(icmp);
                }
                outputs.push(ServerEntry {
                    protocol: query.protocol.clone(),
                    server,
                });
            }
            Outcome::FollowUp(q) => follow_ups.push(q),
        }
    }

    Ok(Run {
        outputs,
        follow_ups,
    })
}

/// Builder for [`Client`].
pub struct ClientBuilder {
    resolver: Arc<dyn Resolver>,
    pinger: Arc<dyn Pinger>,
    concurrency: usize,
    timeout: Duration,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            resolver: Arc::new(
                hickory_resolver::TokioResolver::builder_tokio()
                    .unwrap()
                    .build()
                    .unwrap(),
            ) as Arc<dyn Resolver>,
            pinger: Arc::new(()),
            concurrency: 256,
            timeout: Duration::from_secs(5),
        }
    }
}

impl ClientBuilder {
    pub fn resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = resolver;
        self
    }

    pub fn pinger(mut self, pinger: impl Into<Arc<dyn Pinger>>) -> Self {
        self.pinger = pinger.into();
        self
    }

    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build(self) -> Client {
        Client {
            resolver: self.resolver,
            pinger: self.pinger,
            concurrency: self.concurrency,
            timeout: self.timeout,
        }
    }
}
