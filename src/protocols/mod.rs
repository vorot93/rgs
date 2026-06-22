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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FollowUpQueryProtocol, Packet, ParseResult};
    use futures::StreamExt;

    #[test]
    fn default_protocols_contains_q3m_and_q3s() {
        let protocols = make_default_protocols();
        assert!(protocols.contains_key("q3m"));
        assert!(protocols.contains_key("q3s"));
    }

    #[test]
    fn q3m_follow_ups_target_the_shared_q3s_protocol() {
        let protocols = make_default_protocols();
        let q3m = &protocols["q3m"];
        let q3s = &protocols["q3s"];

        // A master response with one server address.
        let mut data = Vec::new();
        q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
            data: ["10.0.0.1:27960".parse().unwrap()].into_iter().collect(),
        })
        .write_bytes(&mut data)
        .unwrap();
        data.extend_from_slice(b"\\EOT");

        let stream = q3m.parse_response(Packet {
            addr: "5.6.7.8:27950".parse().unwrap(),
            data,
        });
        let results = futures::executor::block_on(stream.collect::<Vec<_>>());
        assert_eq!(results.len(), 1);

        match results.into_iter().next().unwrap().unwrap() {
            // The follow-up must reuse the very same q3s protocol registered in the map
            // (pointer identity), proving the master is wired to its child.
            ParseResult::FollowUp(f) => {
                assert_eq!(f.protocol, FollowUpQueryProtocol::Child(q3s.clone()));
            }
            other => panic!("expected FollowUp, got {other:?}"),
        }
    }
}
