pub mod a2s;
pub mod q3m;
pub mod q3s;

use crate::models::TProtocol;

use std::collections::HashMap;

pub fn make_default_protocols() -> HashMap<String, TProtocol> {
    let mut out = HashMap::new();

    let q3s_proto = TProtocol::from(q3s::ProtocolImpl::default());
    let q3m_proto = TProtocol::from(q3m::ProtocolImpl {
        q3s_protocol: Some(q3s_proto.clone()),
        ..Default::default()
    });

    out.insert("q3s".into(), q3s_proto);
    out.insert("q3m".into(), q3m_proto);

    out
}
