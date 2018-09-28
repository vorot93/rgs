use failure;
use futures::{future, prelude::*};
use rand::random;
use std::net::IpAddr;
use std::time::Duration;
use tokio_ping;

pub trait Pinger: Send + Sync {
    fn ping(
        &self,
        addr: IpAddr,
    ) -> Box<Future<Item = Option<Duration>, Error = failure::Error> + Send + Sync>;
}

pub struct DummyPinger;

impl Pinger for DummyPinger {
    fn ping(
        &self,
        _addr: IpAddr,
    ) -> Box<Future<Item = Option<Duration>, Error = failure::Error> + Send + Sync> {
        Box::new(future::ok(None))
    }
}

impl Pinger for tokio_ping::Pinger {
    fn ping(
        &self,
        addr: IpAddr,
    ) -> Box<Future<Item = Option<Duration>, Error = failure::Error> + Send + Sync> {
        Box::new(
            self.ping(addr, random(), 0, Duration::from_secs(4))
                .map(|rtt| rtt.map(|v| Duration::from_millis(v as u64)))
                .map_err(failure::Error::from),
        )
    }
}
