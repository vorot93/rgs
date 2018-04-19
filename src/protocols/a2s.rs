use futures::prelude::*;
use serde_json::Value;

use errors::Error;
use protocols::models::{Packet, ParseResult, Protocol};

#[derive(Debug)]
pub struct A2SProtocol {}

impl Protocol for A2SProtocol {
    fn make_request(&self, state: Option<Value>) -> Vec<u8> {
        let mut out = Vec::<u8>::new();
        out.extend_from_slice(&[4, 4, 4, 4]);
        out.extend_from_slice(String::from("TSource Engine Query").as_bytes());
        out.push(0);

        out
    }
    fn parse_response(&self, p: Packet) -> Box<Stream<Item = ParseResult, Error = Error>> {
        unimplemented!()
    }
}
