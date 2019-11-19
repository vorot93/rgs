use {
    log::debug,
    rgs::models::*,
    std::sync::{Arc, Mutex},
    tokio::prelude::*,
};

fn main() {
    env_logger::init();

    let pconfig = rgs::protocols::make_default_protocols();

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
        rgs::UdpQuery::simple_query(requests)
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
    ) as Box<dyn Future<Item = (), Error = ()> + Send>;

    debug!("Starting reactor");
    tokio::run(task);
    debug!("Queried {} servers", total_queried.lock().unwrap());
}
