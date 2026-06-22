use crate::{
    model::{Host, Query},
    protocol::{Outcome, Parsed, Protocol},
};
use anyhow::format_err;
use q3a::MasterQueryExtra::*;
use std::{net::SocketAddr, sync::Arc};

#[derive(Clone, Debug)]
pub struct Q3m {
    pub q3s_protocol: Option<Arc<dyn Protocol>>,
    pub request_tag: Option<String>,
    pub version: u32,
}

impl Default for Q3m {
    fn default() -> Self {
        Self {
            q3s_protocol: None,
            request_tag: None,
            version: 71,
        }
    }
}

/// Real masters terminate the (possibly multi-datagram) server list with `\EOT`.
fn contains_eot(data: &[u8]) -> bool {
    data.windows(4).any(|w| w == b"\\EOT")
}

impl Protocol for Q3m {
    fn make_request(&self) -> Vec<u8> {
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

    fn parse_response(&self, _addr: SocketAddr, data: &[u8]) -> anyhow::Result<Parsed> {
        let expect_more = !contains_eot(data);
        let (_, pkt) = q3a::Packet::from_bytes(data).map_err(|e| format_err!("{e:?}"))?;
        match pkt {
            q3a::Packet::GetServersResponse(resp) => {
                let outcomes = match self.q3s_protocol.clone() {
                    Some(q3s_protocol) => resp
                        .data
                        .into_iter()
                        .map(|addr| {
                            Outcome::FollowUp(Query {
                                protocol: q3s_protocol.clone(),
                                host: Host::Addr(SocketAddr::V4(addr)),
                            })
                        })
                        .collect(),
                    None => vec![],
                };
                Ok(Parsed {
                    outcomes,
                    expect_more,
                })
            }
            other => Err(format_err!("Wrong packet type: {:?}", other.get_type())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::q3s;
    use std::{
        collections::HashSet,
        net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    };

    fn packet_addr() -> SocketAddr {
        "5.6.7.8:27950".parse().unwrap()
    }

    /// A valid, terminated `getserversResponse` (q3a's writer omits the trailing
    /// `\EOT` that its own parser requires, so append it).
    fn getservers_response_bytes(addrs: &[SocketAddrV4]) -> Vec<u8> {
        let mut out = Vec::new();
        q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
            data: addrs.iter().copied().collect(),
        })
        .write_bytes(&mut out)
        .unwrap();
        out.extend_from_slice(b"\\EOT");
        out
    }

    #[test]
    fn make_request_builds_getservers() {
        // Assert on the raw request bytes rather than round-tripping: `extra` is a
        // HashSet, so flags may be written in an order the parser does not accept.
        let bytes = Q3m::default().make_request();
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.contains("getservers"), "request: {text:?}");
        assert!(text.contains("71"), "request: {text:?}");
        assert!(text.contains("empty"), "request: {text:?}");
        assert!(text.contains("full"), "request: {text:?}");
    }

    #[test]
    fn parse_response_emits_followups_for_each_server() {
        let child: Arc<dyn Protocol> = Arc::new(q3s::Q3s::default());
        let proto = Q3m {
            q3s_protocol: Some(child.clone()),
            ..Default::default()
        };

        let addrs = [
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960),
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 27961),
        ];

        let parsed = proto
            .parse_response(packet_addr(), &getservers_response_bytes(&addrs))
            .unwrap();
        // The terminating packet carries `\EOT`, so no more datagrams are expected.
        assert!(!parsed.expect_more);
        assert_eq!(parsed.outcomes.len(), addrs.len());

        let mut seen = HashSet::new();
        for outcome in parsed.outcomes {
            match outcome {
                Outcome::FollowUp(q) => {
                    // The follow-up must reuse the very same q3s protocol (pointer identity).
                    assert!(Arc::ptr_eq(&q.protocol, &child));
                    match q.host {
                        Host::Addr(addr) => seen.insert(addr),
                        other => panic!("expected resolved address, got {other:?}"),
                    };
                }
                other => panic!("expected FollowUp, got {other:?}"),
            }
        }

        let expected: HashSet<SocketAddr> = addrs.iter().map(|a| SocketAddr::V4(*a)).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn parse_response_without_child_protocol_yields_nothing() {
        let proto = Q3m::default(); // q3s_protocol is None
        let addrs = [SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960)];
        let parsed = proto
            .parse_response(packet_addr(), &getservers_response_bytes(&addrs))
            .unwrap();
        assert!(parsed.outcomes.is_empty());
    }

    #[test]
    fn parse_response_rejects_wrong_packet_type() {
        // A GetServers request is not a valid response.
        let data = Q3m::default().make_request();
        assert!(Q3m::default().parse_response(packet_addr(), &data).is_err());
    }

    #[test]
    fn parse_response_requires_eot_terminator() {
        // q3a's parser needs the trailing `\EOT`; without it the datagram cannot
        // be parsed, so `parse_response` returns an error.
        let mut data = Vec::new();
        q3a::Packet::GetServersResponse(q3a::GetServersResponseData {
            data: [SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 27960)]
                .into_iter()
                .collect(),
        })
        .write_bytes(&mut data)
        .unwrap();
        // deliberately omit the `\EOT` terminator
        assert!(Q3m::default().parse_response(packet_addr(), &data).is_err());
    }
}
