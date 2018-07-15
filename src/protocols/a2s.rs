use serde_json::Value;

use models::{Packet, Protocol, ProtocolResultStream};

#[derive(Debug)]
pub struct A2SProtocol {}

impl Protocol for A2SProtocol {
    fn make_request(&self, _: Option<Value>) -> Vec<u8> {
        let mut out = Vec::<u8>::new();
        out.extend_from_slice(&[4, 4, 4, 4]);
        out.extend_from_slice(String::from("TSource Engine Query").as_bytes());
        out.push(0);

        out
    }
    fn parse_response(&self, _p: Packet) -> ProtocolResultStream {
        unimplemented!()
    }
}
