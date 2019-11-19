use {
    futures01::{future, prelude::*},
    rand::random,
    std::net::IpAddr,
    std::time::Duration,
};

pub trait Pinger: Send + Sync {
    fn ping(
        &self,
        addr: IpAddr,
    ) -> Box<dyn Future<Item = Option<Duration>, Error = failure::Error> + Send>;
}

pub struct DummyPinger;

impl Pinger for DummyPinger {
    fn ping(
        &self,
        _addr: IpAddr,
    ) -> Box<dyn Future<Item = Option<Duration>, Error = failure::Error> + Send> {
        Box::new(future::ok(None))
    }
}

impl Pinger for tokio_ping::Pinger {
    fn ping(
        &self,
        addr: IpAddr,
    ) -> Box<dyn Future<Item = Option<Duration>, Error = failure::Error> + Send> {
        Box::new(
            self.ping(addr, random(), 0, Duration::from_secs(4))
                .map_err(failure::Error::from),
        )
    }
}
