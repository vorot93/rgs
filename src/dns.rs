extern crate futures_await as futures;
extern crate std;
extern crate tokio_dns;

use errors;
use errors::Error;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use protocols;
use protocols::models as pmodels;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[async]
pub fn resolve_host(
    resolver: Arc<tokio_dns::Resolver + Send + Sync + 'static>,
    host: pmodels::Host,
) -> errors::Result<SocketAddr> {
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

pub type History = Arc<Mutex<HashMap<SocketAddr, String>>>;

pub struct ResolvedQuery {
    pub addr: SocketAddr,
    pub protocol: pmodels::TProtocol,
}

pub struct Resolver {
    inner: Arc<tokio_dns::Resolver + Send + Sync + 'static>,
    history: History,
    pending_requests: FuturesUnordered<Box<Future<Item = Option<ResolvedQuery>, Error = Error>>>,
}

impl Resolver {
    pub fn new<R>(resolver: Arc<R>, history: History) -> Self
    where
        R: tokio_dns::Resolver + Send + Sync + 'static,
    {
        let mut pending_requests = FuturesUnordered::new();
        pending_requests.push(futures::future::empty());
        Self {
            inner: resolver,
            history,
            pending_requests,
        }
    }
}

impl Sink for Resolver {
    type SinkItem = pmodels::Query;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.pending_requests.push(
            resolve_host(self.inner, item.addr)
                .inspect({
                    let host = item.addr.clone();
                    let history = Arc::clone(self.history);
                    move |&addr| {
                        if let pmodels::Host::S(ref s) = *addr {
                            history.insert(addr.clone(), s.clone());
                        }
                    }
                })
                .map(|q| {
                    Some(ResolvedQuery {
                        addr: q.addr,
                        protocol: q.protocol,
                    })
                })
                .or_else(|e| Ok(None)),
        );
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}

impl Stream for Resolver {
    type Item = ResolvedQuery;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.pending_requests.poll()
    }
}
