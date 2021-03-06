use crate::{errors::Error, models::*};

use {
    failure::format_err,
    futures01::{prelude::*, stream::FuturesUnordered},
    log::debug,
    serde_json::Value,
    std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{Arc, Mutex},
    },
};

pub trait Resolver: Send + Sync + 'static {
    fn resolve(
        &self,
        host: Host,
    ) -> Box<dyn Future<Item = SocketAddr, Error = failure::Error> + Send>;
}

impl<T> Resolver for T
where
    T: tokio_dns::Resolver + Send + Sync + 'static,
{
    fn resolve(
        &self,
        host: Host,
    ) -> Box<dyn Future<Item = SocketAddr, Error = failure::Error> + Send> {
        match host {
            Host::A(addr) => Box::new(futures01::future::ok(addr)),
            Host::S(stringaddr) => Box::new(
                tokio_dns::Resolver::resolve(self, &stringaddr.host)
                    .map_err(|e| e.into())
                    .and_then(move |addrs| {
                        addrs
                            .into_iter()
                            .next()
                            .map(|ipaddr| SocketAddr::new(ipaddr, stringaddr.port))
                            .ok_or_else(|| {
                                format_err!("Failed to resolve host {}", &stringaddr.host)
                                    .context(Error::NetworkError)
                                    .into()
                            })
                    }),
            ),
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
    pending_requests: FuturesUnordered<
        Box<dyn Future<Item = Option<ResolvedQuery>, Error = failure::Error> + Send>,
    >,
}

impl ResolverPipe {
    pub fn new(resolver: Arc<dyn Resolver>, history: History) -> Self {
        let mut pending_requests = FuturesUnordered::new();
        pending_requests.push(Box::new(futures01::future::empty())
            as Box<
                dyn Future<Item = Option<ResolvedQuery>, Error = failure::Error> + Send,
            >);
        Self {
            inner: resolver,
            history,
            pending_requests,
        }
    }
}

impl Sink for ResolverPipe {
    type SinkItem = Query;
    type SinkError = failure::Error;

    fn start_send(&mut self, query: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.pending_requests.push(Box::new(
            self.inner
                .resolve(query.host.clone())
                .inspect({
                    let host = query.host.clone();
                    let history = Arc::clone(&self.history);
                    move |&addr| {
                        if let Host::S(ref s) = host {
                            history.lock().unwrap().insert(addr, s.host.clone());
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
                .or_else(|_e| Ok(None)),
        ));
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }
}

impl Stream for ResolverPipe {
    type Item = ResolvedQuery;
    type Error = failure::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Async::Ready(Some(result)) = self.pending_requests.poll()? {
            if let Some(resolved) = result {
                if resolved.addr.ip().is_unspecified() {
                    debug!("Ignoring unspecified address");
                } else {
                    debug!("Resolved: {:?}", resolved.addr);
                    return Ok(Async::Ready(Some(resolved)));
                }
            }
        }

        Ok(Async::NotReady)
    }
}
