extern crate failure;
extern crate futures;
extern crate futures_timer;
extern crate librgs;
extern crate rand;
extern crate resolve;
extern crate serde_json;
extern crate tokio;

use futures::prelude::*;
use futures_timer::FutureExt;
use librgs::models::*;
use librgs::util::LoggingService;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;

fn main() {
    let logger = librgs::util::RealLogger;
    let pconfig = librgs::protocols::make_default_protocols();

    let requests = vec![UserQuery {
        protocol: pconfig.get("openttdm".into()).unwrap().clone(),
        host: Host::S(
            StringAddr {
                host: "master.openttd.org".into(),
                port: 3978,
            }.into(),
        ),
    }];

    let query_builder = librgs::UdpQueryBuilder::default();

    let socket = UdpSocket::bind(&SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        5678,
    )).unwrap();
    let (request_sink, server_stream) = query_builder.build(socket).split();

    let request_stream = Box::new(futures::stream::iter_ok::<Vec<UserQuery>, failure::Error>(
        requests,
    )) as Box<Stream<Item = UserQuery, Error = failure::Error> + Send>;
    let request_fut = Box::new(request_sink.send_all(request_stream).and_then(|_| {
        println!("Sent all");
        Ok(())
    })) as Box<Future<Item = (), Error = failure::Error> + Send>;

    let timeout = std::time::Duration::from_secs(10);

    let task = Box::new(
        futures::future::join_all(vec![
            request_fut as Box<Future<Item = (), Error = failure::Error> + Send>,
            Box::new(
                server_stream
                    .inspect(move |entry| {
                        logger.info(&serde_json::to_string(&entry.clone().into_inner().1).unwrap());
                    })
                    .for_each(|_| Ok(())),
            ) as Box<Future<Item = (), Error = failure::Error> + Send>,
        ]).timeout(timeout)
            .map(|_| ())
            .map_err(|_| ()),
    ) as Box<Future<Item = (), Error = ()> + Send>;

    println!("Starting reactor");
    tokio::run(task);
}
