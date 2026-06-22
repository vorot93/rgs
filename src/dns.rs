use crate::model::Host;
use anyhow::format_err;
use std::net::{IpAddr, SocketAddr};

/// Resolve a [`Host`] to a single socket address.
///
/// `Host::Addr` is returned unchanged. A named host is resolved and an IPv4
/// result is preferred (q3 servers are IPv4), falling back to the first address.
pub async fn resolve(
    resolver: &hickory_resolver::TokioResolver,
    host: Host,
) -> anyhow::Result<SocketAddr> {
    match host {
        Host::Addr(addr) => Ok(addr),
        Host::Named { host, port } => {
            let lookup = resolver.lookup_ip(&host).await?;
            let ip = lookup
                .iter()
                .find(IpAddr::is_ipv4)
                .or_else(|| lookup.iter().next())
                .ok_or_else(|| format_err!("Failed to resolve host {host}"))?;
            Ok(SocketAddr::new(ip, port))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_returns_literal_address_unchanged() {
        let resolver = hickory_resolver::TokioResolver::builder_tokio()
            .unwrap()
            .build()
            .unwrap();
        let addr: SocketAddr = "9.9.9.9:27960".parse().unwrap();
        assert_eq!(resolve(&resolver, Host::Addr(addr)).await.unwrap(), addr);
    }
}
