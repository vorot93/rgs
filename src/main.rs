extern crate env_logger;
extern crate failure;
extern crate futures;
extern crate futures_timer;
extern crate librgs;
#[macro_use]
extern crate log;
extern crate rand;
extern crate resolve;
extern crate serde_json;
extern crate tokio;

use env_logger::Builder as EnvLogBuilder;
use futures::prelude::*;
use futures_timer::StreamExt;
use librgs::models::*;
use std::env;
use std::sync::{Arc, Mutex};

fn init_logging() {
    let mut builder = EnvLogBuilder::new();
    if let Ok(v) = env::var("RUST_LOG") {
        builder.parse(&v);
    }
    let stdio_logger = builder.build();
    let log_level = stdio_logger.filter();
    log::set_max_level(log_level);
    log::set_boxed_logger(Box::new(stdio_logger)).expect("Failed to install logger");
}

fn main() {
    init_logging();

    let pconfig = librgs::protocols::make_default_protocols();

    let requests = vec![
        UserQuery {
            protocol: pconfig["openttdm"].clone(),
            host: Host::S(StringAddr {
                host: "master.openttd.org".into(),
                port: 3978,
            }),
        },
        UserQuery {
            protocol: pconfig["q3m"].clone(),
            host: Host::S(StringAddr {
                host: "master3.idsoftware.com".into(),
                port: 27950,
            }),
        },
    ];

    let timeout = std::time::Duration::from_secs(5);

    let total_queried = Arc::new(Mutex::new(0));

    let task = Box::new(
        librgs::simple_udp_query(requests)
            .inspect({
                let total_queried = total_queried.clone();
                move |entry| {
                    debug!("{:?}", entry);
                    *total_queried.lock().unwrap() += 1;
                }
            })
            .map_err(|e| {
                debug!("UdpQuery returned an error: {:?}", e);
                e
            })
            .timeout(timeout)
            .for_each(|_| Ok(()))
            .map(|_| ())
            .map_err(|_| ()),
    ) as Box<Future<Item = (), Error = ()> + Send>;

    debug!("Starting reactor");
    tokio::run(task);
    debug!("Queried {} servers", total_queried.lock().unwrap());
}
