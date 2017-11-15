extern crate librgs;
extern crate rand;
extern crate resolve;
extern crate serde_json;
extern crate rgs_models as models;
extern crate enum_primitive;

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
        pmodels::QueryEntry {
            protocol: pconfig.get("openttds".into()).unwrap().clone(),
            addr: pmodels::Host::S(
                pmodels::StringAddr {
                    host: "ttd.duck.me.uk".into(),
                    port: 3979,
                }.into(),
            ),
        },
    ];

    let query_manager = librgs::RealQueryManager::new(
        logger.clone(),
        Box::new(std::io::stdout()),
    );

    let data = query_manager.query_udp(requests);
    logger.info(&format!("{:?}", data));
}
