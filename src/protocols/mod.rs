pub mod helpers;
pub mod models;

pub mod openttdm;
pub mod openttds;
pub mod q3s;

use errors;
use models::{Protocol, TProtocol};
use std::collections::HashMap;
use std::sync::Arc;

pub fn make_default_protocols() -> HashMap<String, TProtocol> {
    let mut out = HashMap::new();

    let openttds_proto = TProtocol::from(Arc::new(openttds::Protocol) as Arc<Protocol + 'static>);
    out.insert("openttds".into(), openttds_proto.clone());

    let openttdm_proto = TProtocol::from(Arc::new(openttdm::ProtocolImpl {
        child: Some(openttds_proto.clone()),
    }) as Arc<Protocol + 'static>);
    out.insert("openttdm".into(), openttdm_proto);

    out
}
