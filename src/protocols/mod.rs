pub mod a2s;
pub mod openttdm;
pub mod openttds;
pub mod q3m;
pub mod q3s;

use models::TProtocol;
use std::collections::HashMap;

pub fn make_default_protocols() -> HashMap<String, TProtocol> {
    let mut out = HashMap::new();

    let openttds_proto = TProtocol::from(openttds::ProtocolImpl);
    let openttdm_proto = openttdm::ProtocolImpl {
        child: Some(openttds_proto.clone()),
    }.into();

    let q3_ver = 68;
    let q3s_proto = TProtocol::from(q3s::ProtocolImpl {
        version: q3_ver,
        ..Default::default()
    });
    let q3m_proto = q3m::ProtocolImpl {
        q3s_protocol: Some(q3s_proto.clone()),
        version: u32::from(q3_ver),
    }.into();

    out.insert("openttds".into(), openttds_proto);
    out.insert("openttdm".into(), openttdm_proto);
    out.insert("q3s".into(), q3s_proto);
    out.insert("q3m".into(), q3m_proto);

    out
}
