extern crate futures_await as futures;
extern crate librgs;
extern crate rand;
extern crate resolve;
extern crate serde_json;
extern crate tokio_core;
extern crate tokio_timer;

use tokio_core::net::UdpSocket;
use futures::prelude::*;
use librgs::util::LoggingService;
use librgs::protocols::models::*;
use librgs::errors::Error;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn main() {
    // let server = ("master.openttd.org", 3978);
    // let p = protocols::openttdm::P::default();
    let logger = librgs::util::RealLogger;
    let mut pconfig = librgs::protocols::make_default_protocols();

    let requests = vec![
        UserQuery {
            protocol: pconfig.get("openttdm".into()).unwrap().clone(),
            host: Host::S(
                StringAddr {
                    host: "master.openttd.org".into(),
                    port: 3978,
                }.into(),
            ),
        },
    ];

    let query_builder = librgs::UdpQueryBuilder::new();

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let socket = UdpSocket::bind(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 5678),
        &core.handle(),
    ).unwrap();
    let (request_sink, server_stream) = query_builder.make_query(socket).split();

    let request_stream = futures::stream::iter_ok::<Vec<UserQuery>, Error>(requests);
    let request_fut = request_sink.send_all(request_stream).and_then(|_| {
        println!("Sent all");
        Ok(())
    });

    let timer = tokio_timer::Timer::default();

    let timeout = std::time::Duration::from_secs(10);

    println!("Starting core");

    core.run(timer.timeout(
        futures::future::join_all(vec![
        Box::new(request_fut) as Box<Future<Item = (), Error = Error>>,
        Box::new(
            server_stream
                .inspect(move |data| {
                    logger.info(&format!("{:?}", data));
                })
                .for_each(|_| Ok(())),
        ),
    ]),
        timeout,
    ));
}
