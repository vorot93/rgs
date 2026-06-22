use futures::future::BoxFuture;
use rand::random;
use std::{future::ready, net::IpAddr, time::Duration};
use surge_ping::PingIdentifier;

pub trait Pinger: Send + Sync {
    fn ping(&self, addr: IpAddr) -> BoxFuture<'static, anyhow::Result<Option<Duration>>>;
}

impl Pinger for () {
    fn ping(&self, _: IpAddr) -> BoxFuture<'static, anyhow::Result<Option<Duration>>> {
        Box::pin(ready(Ok(None)))
    }
}

impl Pinger for surge_ping::Client {
    fn ping(&self, addr: IpAddr) -> BoxFuture<'static, anyhow::Result<Option<Duration>>> {
        let client = self.clone();

        Box::pin(async move {
            let mut pinger = client.pinger(addr, PingIdentifier(random())).await;
            let (_, duration) = pinger.ping(0.into(), &[]).await?;

            Ok(Some(duration))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unit_pinger_reports_no_measurement() {
        // The `()` pinger is the no-op default: it never measures a round-trip.
        let result = ().ping("127.0.0.1".parse::<IpAddr>().unwrap()).await.unwrap();
        assert_eq!(result, None);
    }
}
