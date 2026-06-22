use crate::{
    errors::Error,
    models::{
        FollowUpQuery, FollowUpQueryProtocol, Packet, ParseResult, Protocol, ProtocolResultStream,
        TProtocol,
    },
};
use anyhow::format_err;
use futures::prelude::*;
use q3a::MasterQueryExtra::*;
use serde_json::Value;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct ProtocolImpl {
    pub q3s_protocol: Option<TProtocol>,
    pub request_tag: Option<String>,
    pub version: u32,
}

impl Default for ProtocolImpl {
    fn default() -> Self {
        Self {
            q3s_protocol: None,
            request_tag: None,
            version: 71,
        }
    }
}

impl Protocol for ProtocolImpl {
    /// Creates a request packet. Can accept an optional state if there is any.
    fn make_request(&self, _: Option<Value>) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetServers(q3a::GetServersData {
            request_tag: self.request_tag.clone(),
            version: self.version,
            extra: vec![Empty, Full].into_iter().collect(),
        })
        .write_bytes(&mut out)
        .unwrap();
        out
    }
    /// Create a stream of parsed values out of incoming response.
    fn parse_response(&self, p: Packet) -> ProtocolResultStream {
        Box::pin(
            futures::stream::iter(
                match q3a::Packet::from_bytes(p.data.as_slice())
                    .map_err(|e| format_err!("{e:?}"))
                    .and_then(|(_, pkt)| match pkt {
                        q3a::Packet::GetServersResponse(data) => {
                            Ok(match self.q3s_protocol.clone() {
                                Some(q3s_protocol) => data
                                    .data
                                    .into_iter()
                                    .map(|addr| {
                                        ParseResult::FollowUp(FollowUpQuery {
                                            host: SocketAddr::V4(addr).into(),
                                            state: None,
                                            protocol: FollowUpQueryProtocol::Child(
                                                q3s_protocol.clone(),
                                            ),
                                        })
                                    })
                                    .collect(),
                                None => vec![],
                            })
                        }
                        other => Err(format_err!("Wrong packet type: {:?}", other.get_type())
                            .context(Error::DataParseError)),
                    }) {
                    Ok(servers) => servers.into_iter().map(Ok).collect(),
                    Err(e) => vec![Err(e)],
                },
            )
            .map_err({
                let p = p.clone();
                move |e| (Some(p.clone()), e)
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::q3s;
    use futures::StreamExt;
    use std::{
        collections::HashSet,
        net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    };

    fn packet_addr() -> SocketAddr {
        "5.6.7.8:27950".parse().unwrap()
    }

    fn getservers_response_bytes(addrs: &[SocketAddrV4]) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
            data: addrs.iter().copied().collect(),
        })
        .write_bytes(&mut out)
        .unwrap();
        // q3a's writer omits the trailing `\EOT` terminator that real masters send
        // (and that its own parser requires), so append it to form valid wire bytes.
        out.extend_from_slice(b"\\EOT");
        out
    }

    fn parse(proto: &ProtocolImpl, data: Vec<u8>) -> Vec<ParseResult> {
        let stream = proto.parse_response(Packet {
            addr: packet_addr(),
            data,
        });
        futures::executor::block_on(stream.collect::<Vec<_>>())
            .into_iter()
            .map(|r| r.expect("expected a successful parse result"))
            .collect()
    }

    #[test]
    fn make_request_builds_getservers() {
        // Assert on the raw request bytes rather than round-tripping through q3a's
        // parser: `extra` is a HashSet, so its flags can be written in an order the
        // parser does not accept, making a parse round-trip unreliable here.
        let bytes = ProtocolImpl::default().make_request(None);
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.contains("getservers"), "request: {text:?}");
        assert!(text.contains("71"), "request: {text:?}");
        assert!(text.contains("empty"), "request: {text:?}");
        assert!(text.contains("full"), "request: {text:?}");
    }

    #[test]
    fn parse_response_emits_followups_for_each_server() {
        let child = TProtocol::from(q3s::ProtocolImpl::default());
        let proto = ProtocolImpl {
            q3s_protocol: Some(child.clone()),
            ..Default::default()
        };

        let addrs = [
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960),
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 27961),
        ];

        let results = parse(&proto, getservers_response_bytes(&addrs));
        assert_eq!(results.len(), addrs.len());

        let mut seen = HashSet::new();
        for result in results {
            let follow_up = match result {
                ParseResult::FollowUp(f) => f,
                other => panic!("expected FollowUp, got {other:?}"),
            };
            assert_eq!(
                follow_up.protocol,
                FollowUpQueryProtocol::Child(child.clone())
            );
            match follow_up.host {
                crate::models::Host::A(addr) => seen.insert(addr),
                other => panic!("expected resolved address, got {other:?}"),
            };
        }

        let expected: HashSet<SocketAddr> = addrs.iter().map(|a| SocketAddr::V4(*a)).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn parse_response_without_child_protocol_yields_nothing() {
        let proto = ProtocolImpl::default(); // q3s_protocol is None
        let addrs = [SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960)];
        assert!(parse(&proto, getservers_response_bytes(&addrs)).is_empty());
    }

    #[test]
    fn parse_response_rejects_wrong_packet_type() {
        // A GetServers request is not a valid response.
        let data = ProtocolImpl::default().make_request(None);
        let stream = ProtocolImpl::default().parse_response(Packet {
            addr: packet_addr(),
            data,
        });
        let results = futures::executor::block_on(stream.collect::<Vec<_>>());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }
}
