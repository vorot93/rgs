use errors::Error;
use models::*;

use failure;
use futures;
use openttd;
use serde_json::Value;
use std::collections::HashSet;
use std::net::SocketAddr;

fn parse_data(buf: &[u8]) -> Result<HashSet<SocketAddr>, failure::Error> {
    let p = openttd::Packet::from_incoming_bytes(buf)
        .map_err(|e| format_err!("Nom failure while parsing: {}", e))?
        .1;

    match p {
        openttd::Packet::MasterResponseList(data) => Ok(match data {
            openttd::ServerList::IPv4(set) => set.into_iter().map(SocketAddr::V4).collect::<_>(),
            openttd::ServerList::IPv6(set) => set.into_iter().map(SocketAddr::V6).collect::<_>(),
        }),
        _ => Err(format_err!("invalid packet type: {:?}", p.pkt_type())
            .context(Error::DataParseError)
            .into()),
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

    fn parse_response(&self, pkt: Packet) -> ProtocolResultStream {
        if let Some(child) = self.child.clone() {
            let data = pkt.data;

            Box::new(futures::stream::iter_result(match parse_data(&data) {
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
            }))
        } else {
            Box::new(futures::stream::iter_ok(vec![]))
        }
    }
}
