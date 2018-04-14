use errors::Error;
use models::*;
use util;

use byteorder::LittleEndian;
use futures;
use futures::prelude::*;
use num_traits::FromPrimitive;
use openttd;
use serde_json::Value;
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

fn parse_data(addr: SocketAddr, buf: &[u8]) -> Result<HashSet<SocketAddr>, Error> {
    let p = openttd::Packet::from_incoming_bytes(buf)?.1;

    match p {
        openttd::Packet::MasterResponseList(data) => Ok(match data {
            openttd::ServerList::IPv4(set) => set.into_iter().map(SocketAddr::V4).collect::<_>(),
            openttd::ServerList::IPv6(set) => set.into_iter().map(SocketAddr::V6).collect::<_>(),
        }),
        _ => Err(Error::DataParseError {
            reason: format!("invalid packet type: {:?}", p.pkt_type()),
        }),
    }
}

#[derive(Debug)]
pub struct ProtocolImpl {
    pub child: Option<TProtocol>,
}

impl Protocol for ProtocolImpl {
    fn make_request(&self, _state: Option<Value>) -> Vec<u8> {
        openttd::Packet::ClientGetList(openttd::ClientGetListData {
            master_server_version: 2,
            request_type: openttd::ServerListType::Autodetect,
        }).to_bytes()
            .unwrap()
    }

    fn parse_response(&self, pkt: Packet) -> Box<Stream<Item = ParseResult, Error = Error> + Send> {
        if let Some(child) = self.child.clone() {
            let Packet { data, addr } = pkt;

            Box::new(futures::stream::iter_result(
                match parse_data(addr, &data) {
                    Err(e) => vec![Err(e)],
                    Ok(data) => data.into_iter()
                        .map(move |addr| {
                            Ok(ParseResult::FollowUp(FollowUpQuery {
                                host: Host::A(addr),
                                protocol: FollowUpQueryProtocol::Child(child.clone()),
                                state: None,
                            }))
                        })
                        .collect::<Vec<_>>(),
                },
            ))
        } else {
            Box::new(futures::stream::iter_ok(vec![]))
        }
    }
}
