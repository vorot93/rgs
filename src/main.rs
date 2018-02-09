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

use futures::prelude::*;
use librgs::QueryService;
use librgs::util::LoggingService;
use librgs::protocols::models as pmodels;
use serde_json::Value;

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
        pmodels::Query {
            protocol: pconfig.get("openttds".into()).unwrap().clone(),
            host: pmodels::Host::S(
                pmodels::StringAddr {
                    host: "ttd.duck.me.uk".into(),
                    port: 3979,
                }.into(),
            ),
        },
    ];

    let query_manager = librgs::RealQueryService::default();

    let mut core = tokio_core::reactor::Core::new().unwrap();
    core.run(
        query_manager
            .query_udp(Box::new(futures::stream::iter_ok(requests)))
            .inspect(|data| {
                logger.info(&format!("{:?}", data));
            })
            .for_each(|_| Ok(())),
    ).unwrap();
}
