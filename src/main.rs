#![feature(proc_macro)]
#![feature(conservative_impl_trait)]
#![feature(generators)]

extern crate futures_await as futures;
extern crate librgs;
extern crate rand;
extern crate resolve;
extern crate rgs_models as models;
extern crate serde_json;
extern crate tokio_core;

use tokio_core::net::UdpSocket;
use futures::prelude::*;
use librgs::util::LoggingService;
use librgs::protocols::models as pmodels;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn main() {
    // let server = ("master.openttd.org", 3978);
    // let p = protocols::openttdm::P::default();
    let logger = librgs::util::RealLogger;
    let mut pconfig = pmodels::ProtocolConfig::new();

    {
        let server_p = librgs::protocols::make_protocol(
            "openttds",
            &{
                let mut m = pmodels::Config::default();
                m.insert("prelude-finisher".into(), Value::String("\x00\x00".into()));
                m
            },
            None,
        );
        pconfig.insert("openttds".into(), server_p.unwrap().unwrap());
    }

    let requests = vec![
        pmodels::UserQuery {
            protocol: pconfig.get("openttds".into()).unwrap().clone(),
            host: pmodels::Host::S(
                pmodels::StringAddr {
                    host: "ttd.duck.me.uk".into(),
                    port: 3979,
                }.into(),
            ),
        },
    ];

    let query_builder = librgs::UdpQueryBuilder::new();

    let mut core = tokio_core::reactor::Core::new().unwrap();
    let socket = UdpSocket::bind(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 5678),
        &core.handle(),
    ).unwrap();
    let (query_sink, server_stream) = query_builder.make_query(socket).split();

    std::thread::spawn(move || {
        let mut core = tokio_core::reactor::Core::new().unwrap();
        core.run(query_sink.send_all(futures::stream::iter_ok(requests)));
    });

    core.run(
        server_stream
            .inspect(|data| {
                logger.info(&format!("{:?}", data));
            })
            .for_each(|_| Ok(())),
    ).unwrap();
}
