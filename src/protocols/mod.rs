pub mod a2s;
pub mod openttdm;
pub mod openttds;
pub mod q3m;
pub mod q3s;

use models::{Protocol, TProtocol};
use std::collections::HashMap;
use std::sync::Arc;

pub fn make_default_protocols() -> HashMap<String, TProtocol> {
    let mut out = HashMap::new();

    let openttds_proto = TProtocol::from(Arc::new(openttds::ProtocolImpl) as Arc<Protocol>);
    let openttdm_proto = TProtocol::from(Arc::new(openttdm::ProtocolImpl {
        child: Some(openttds_proto.clone()),
    }) as Arc<Protocol>);

    let q3_ver = 68;
    let q3s_proto = TProtocol::from(Arc::new(q3s::ProtocolImpl {
        version: q3_ver,
        ..Default::default()
    }) as Arc<Protocol>);
    let q3m_proto = TProtocol::from(Arc::new(q3m::ProtocolImpl {
        q3s_protocol: Some(q3s_proto.clone()),
        version: q3_ver as u32,
    }) as Arc<Protocol>);

    out.insert("openttds".into(), openttds_proto.clone());
    out.insert("openttdm".into(), openttdm_proto);
    out.insert("q3s".into(), q3s_proto);
    out.insert("q3m".into(), q3m_proto);

    out
}
