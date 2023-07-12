use crate::{errors::Error, models::*};
use anyhow::format_err;
use futures::{
    future::{ok, BoxFuture},
    stream::FuturesUnordered,
    Sink, Stream,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::pending,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tracing::*;

pub trait Resolver: Send + Sync + 'static {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>>;
}

impl Resolver for trust_dns_resolver::TokioAsyncResolver {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
        let s = self.clone();
        match host {
            Host::A(addr) => Box::pin(ok(addr)),
            Host::S(stringaddr) => Box::pin(async move {
                let addrs = s.lookup_ip(&stringaddr.host).await?;

                addrs
                    .into_iter()
                    .next()
                    .map(|ipaddr| SocketAddr::new(ipaddr, stringaddr.port))
                    .ok_or_else(|| {
                        format_err!("Failed to resolve host {}", &stringaddr.host)
                            .context(Error::NetworkError)
                    })
            }),
        }
    }
}

pub type History = Arc<Mutex<HashMap<SocketAddr, String>>>;

pub struct ResolvedQuery {
    pub addr: SocketAddr,
    pub protocol: TProtocol,
    pub state: Option<Value>,
}

pub struct ResolverPipe {
    inner: Arc<dyn Resolver>,
    history: History,
    pending_requests: FuturesUnordered<BoxFuture<'static, Option<ResolvedQuery>>>,
}

impl ResolverPipe {
    pub fn new(resolver: Arc<dyn Resolver>, history: History) -> Self {
        let pending_requests = FuturesUnordered::<BoxFuture<'static, Option<ResolvedQuery>>>::new();
        pending_requests.push(Box::pin(pending()));
        Self {
            inner: resolver,
            history,
            pending_requests,
        }
    }
}

impl Sink<Query> for ResolverPipe {
    type Error = anyhow::Error;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, query: Query) -> Result<(), Self::Error> {
        let host = query.host.clone();
        let history = self.history.clone();
        let resolver = self.inner.clone();

        self.pending_requests.push(Box::pin(async move {
            let addr = resolver.resolve(query.host.clone()).await.ok()?;

            if let Host::S(ref s) = host {
                history.lock().unwrap().insert(addr, s.host.clone());
            }

            Some(ResolvedQuery {
                addr,
                protocol: query.protocol,
                state: query.state,
            })
        }));
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Stream for ResolverPipe {
    type Item = ResolvedQuery;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Poll::Ready(Some(Some(resolved))) =
            Pin::new(&mut self.pending_requests).poll_next(cx)
        {
            if resolved.addr.ip().is_unspecified() {
                debug!("Ignoring unspecified address");
            } else {
                debug!("Resolved: {:?}", resolved.addr);
                return Poll::Ready(Some(resolved));
            }
        }

        Poll::Pending
    }
}
