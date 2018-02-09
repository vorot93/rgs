extern crate futures_await as futures;
extern crate serde_json;
extern crate std;
extern crate tokio_dns;

use errors;
use errors::Error;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use protocols;
use protocols::models as pmodels;
use serde_json::Value;
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
                reason: std::error::Error::description(&e).into(),
            })?
            .into_iter()
            .next()
            .map(|ipaddr| SocketAddr::new(ipaddr, stringaddr.port))
            .ok_or_else(|| Error::NetworkError {
                reason: format!("Failed to resolve host {}", &stringaddr.host),
            }),
    }
}

pub type History = Arc<Mutex<HashMap<SocketAddr, String>>>;

pub struct ResolvedQuery {
    pub addr: SocketAddr,
    pub protocol: pmodels::TProtocol,
    pub state: Option<Value>,
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
        pending_requests.push(
            Box::<Future<Item = Option<ResolvedQuery>, Error = Error>>::new(
                futures::future::empty(),
            ),
        );
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

    fn start_send(&mut self, query: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.pending_requests.push(
            resolve_host(self.inner, query.host.clone())
                .inspect({
                    let host = query.host.clone();
                    let history = Arc::clone(&self.history);
                    move |&res| {
                        if let Some(ref addr) = res {
                            if let pmodels::Host::S(ref s) = query.host {
                                history.lock().unwrap().insert(addr.clone(), s.clone());
                            }
                        }
                    }
                })
                .map(|addr| {
                    Some(ResolvedQuery {
                        addr,
                        protocol: query.protocol,
                        state: query.state,
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

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Async::Ready(Some(result)) = self.pending_requests.poll()? {
            if let Some(resolved) = result {
                return Async::Ready(resolved);
            }
        }

        Async::NotReady
    }
}
