use crate::{errors::Error, models::*};
use anyhow::format_err;
use futures::{
    Sink, Stream,
    future::{BoxFuture, ok},
    stream::FuturesUnordered,
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

impl Resolver for hickory_resolver::TokioResolver {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
        let s = self.clone();
        match host {
            Host::A(addr) => Box::pin(ok(addr)),
            Host::S(stringaddr) => Box::pin(async move {
                let addrs = s.lookup_ip(&stringaddr.host).await?;

                addrs
                    .iter()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::q3s;
    use futures::{SinkExt, StreamExt, future::ready};
    use std::time::Duration;

    /// Resolver backed by a static hostname -> address table.
    struct MockResolver {
        table: HashMap<String, SocketAddr>,
    }

    impl Resolver for MockResolver {
        fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
            let result = match host {
                Host::A(addr) => Ok(addr),
                Host::S(s) => self
                    .table
                    .get(&s.host)
                    .copied()
                    .ok_or_else(|| format_err!("unknown host {}", s.host)),
            };
            Box::pin(ready(result))
        }
    }

    fn pipe(table: &[(&str, &str)]) -> (ResolverPipe, History) {
        let resolver = Arc::new(MockResolver {
            table: table
                .iter()
                .map(|(h, a)| (h.to_string(), a.parse().unwrap()))
                .collect(),
        });
        let history = History::default();
        (ResolverPipe::new(resolver, history.clone()), history)
    }

    fn query(host: Host) -> Query {
        Query {
            protocol: TProtocol::from(q3s::ProtocolImpl::default()),
            host,
            state: None,
        }
    }

    #[tokio::test]
    async fn resolves_named_host_and_records_history() {
        let (mut pipe, history) = pipe(&[("example.com", "1.2.3.4:27960")]);

        pipe.send(query(Host::from(("example.com", 27960))))
            .await
            .unwrap();

        let resolved = pipe.next().await.expect("expected a resolved query");
        assert_eq!(resolved.addr, "1.2.3.4:27960".parse().unwrap());

        // The hostname behind the address is recorded for later reverse lookup.
        assert_eq!(
            history
                .lock()
                .unwrap()
                .get(&resolved.addr)
                .map(String::as_str),
            Some("example.com")
        );
    }

    #[tokio::test]
    async fn passes_through_literal_address() {
        let (mut pipe, history) = pipe(&[]);
        let addr: SocketAddr = "9.9.9.9:27960".parse().unwrap();

        pipe.send(query(Host::A(addr))).await.unwrap();

        let resolved = pipe.next().await.expect("expected a resolved query");
        assert_eq!(resolved.addr, addr);
        // Literal addresses carry no hostname, so nothing is recorded.
        assert!(history.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn drops_unspecified_addresses() {
        let (mut pipe, _history) = pipe(&[]);

        pipe.send(query(Host::A("0.0.0.0:27960".parse().unwrap())))
            .await
            .unwrap();

        // Unspecified addresses are filtered out, so nothing is ever yielded.
        let result = tokio::time::timeout(Duration::from_millis(100), pipe.next()).await;
        assert!(result.is_err(), "expected no resolved query for 0.0.0.0");
    }
}
