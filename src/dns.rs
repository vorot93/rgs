use crate::model::Host;
use anyhow::format_err;
use futures::future::{BoxFuture, ok};
use std::net::SocketAddr;

pub trait Resolver: Send + Sync + 'static {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>>;
}

impl Resolver for hickory_resolver::TokioResolver {
    fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
        let s = self.clone();
        match host {
            Host::Addr(addr) => Box::pin(ok(addr)),
            Host::Named { host, port } => Box::pin(async move {
                let addrs = s.lookup_ip(&host).await?;
                addrs
                    .iter()
                    .next()
                    .map(|ipaddr| SocketAddr::new(ipaddr, port))
                    .ok_or_else(|| format_err!("Failed to resolve host {host}"))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::ready;
    use std::collections::HashMap;

    /// Resolver backed by a static hostname -> address table.
    struct MockResolver {
        table: HashMap<String, SocketAddr>,
    }

    impl Resolver for MockResolver {
        fn resolve(&self, host: Host) -> BoxFuture<'static, anyhow::Result<SocketAddr>> {
            let result = match host {
                Host::Addr(addr) => Ok(addr),
                Host::Named { host, .. } => self
                    .table
                    .get(&host)
                    .copied()
                    .ok_or_else(|| format_err!("unknown host {host}")),
            };
            Box::pin(ready(result))
        }
    }

    #[tokio::test]
    async fn resolver_passes_through_literal_address() {
        let resolver = MockResolver {
            table: HashMap::new(),
        };
        let addr: SocketAddr = "9.9.9.9:27960".parse().unwrap();
        assert_eq!(resolver.resolve(Host::Addr(addr)).await.unwrap(), addr);
    }

    #[tokio::test]
    async fn resolver_resolves_named_host() {
        let resolver = MockResolver {
            table: HashMap::from([(
                "example.com".to_string(),
                "1.2.3.4:27960".parse().unwrap(),
            )]),
        };
        let resolved = resolver
            .resolve(Host::from(("example.com", 27960)))
            .await
            .unwrap();
        assert_eq!(resolved, "1.2.3.4:27960".parse().unwrap());
    }
}
