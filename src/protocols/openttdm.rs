use errors;
use errors::Error;
use models;
use util;
use util::*;

use byteorder::{LittleEndian, NetworkEndian};
use futures;
use futures::prelude::*;
use num_traits::FromPrimitive;
use protocols::models as pmodels;
use serde_json;
use serde_json::Value;
use std;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

/// Source: https://git.openttd.org/?p=trunk.git;a=blob;f=src/network/core/udp.h;hb=HEAD#l41
#[derive(Clone, Debug, PartialEq, Primitive)]
enum IPVer {
    V4 = 1,
    V6 = 2,
}

#[derive(Clone, Debug, PartialEq, Primitive)]
enum PktType {
    PacketUdpClientFindServer = 0,
    PacketUdpServerResponse = 1,
    PacketUdpClientDetailInfo = 2,
    PacketUdpServerDetailInfo = 3,
    PacketUdpServerRegister = 4,
    PacketUdpMasterAckRegister = 5,
    PacketUdpClientGetList = 6,
    PacketUdpMasterResponseList = 7,
    PacketUdpServerUnregister = 8,
    PacketUdpClientGetNewgrfs = 9,
    PacketUdpServerNewgrfs = 10,
    PacketUdpMasterSessionKey = 11,
    PacketUdpEnd = 12,
}

#[async_stream(boxed, item=SocketAddrV4)]
fn parse_v4<I>(len: u16, v: I) -> errors::Result<()>
where
    I: std::iter::IntoIterator<Item = u8> + 'static,
{
    let mut it = v.into_iter();
    for _ in 0..len {
        let ip = Ipv4Addr::new(it.next()?, it.next()?, it.next()?, it.next()?);
        let port = util::to_u16::<LittleEndian>(&[it.next()?, it.next()?]);
        stream_yield!(SocketAddrV4::new(ip, port));
    }

    Ok(())
}

#[async_stream(boxed, item=SocketAddrV6)]
fn parse_v6<I>(len: u16, v: I) -> errors::Result<()>
where
    I: std::iter::IntoIterator<Item = u8> + 'static,
{
    let mut it = v.into_iter();

    Ok(())
}

#[async_stream(item=SocketAddr)]
fn parse_data(b: Vec<u8>) -> errors::Result<()> {
    let mut buf = b.into_iter();

    let len = util::to_u16::<LittleEndian>(&[next_item(&mut buf)?, next_item(&mut buf)?]);

    {
        let num = next_item(&mut buf)?;
        let t = PktType::from_u8(num).ok_or_else(|| Error::InvalidPacketError {
            reason: format!("Unknown packet type: {}", num),
        })?;

        if t != PktType::PacketUdpMasterResponseList {
            return Err(Error::InvalidPacketError {
                reason: format!("Invalid packet type: {:?}", t),
            });
        }
    }

    let ip_ver = IPVer::from_u8(next_item(&mut buf)?).ok_or(Error::InvalidPacketError {
        reason: "Unknown IP type".into(),
    })?;

    let host_num = util::to_u16::<LittleEndian>(&[next_item(&mut buf)?, next_item(&mut buf)?]);

    let s: Box<Stream<Item = SocketAddr, Error = Error>> = match ip_ver {
        IPVer::V4 => Box::new(parse_v4(host_num, buf).map(SocketAddr::V4)),
        IPVer::V6 => Box::new(parse_v6(host_num, buf).map(SocketAddr::V6)),
    };

    #[async]
    for e in s {
        stream_yield!(e);
    }

    Ok(())
}

#[derive(Debug)]
pub struct Protocol {
    pub child: Option<pmodels::TProtocol>,
}

impl pmodels::Protocol for Protocol {
    fn make_request(&self, _state: Option<Value>) -> Vec<u8> {
        vec![5, 0, 6, 2, 2]
    }

    fn parse_response(
        &self,
        p: pmodels::Packet,
    ) -> Box<Stream<Item = pmodels::ParseResult, Error = Error>> {
        if let Some(child) = self.child.clone() {
            Box::new(parse_data(p.data).map(move |addr| {
                pmodels::ParseResult::FollowUp(pmodels::FollowUpQuery {
                    host: pmodels::Host::A(addr),
                    protocol: pmodels::FollowUpQueryProtocol::Child(child.clone()),
                    state: None,
                })
            }))
        } else {
            Box::new(futures::stream::iter_ok(vec![]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;
    use std::str::FromStr;

    fn fixtures() -> (Vec<u8>, Vec<SocketAddr>) {
        let data = vec![
            0x42, 0x00, 0x07, 0x01, 0x0A, 0x00, 0x4A, 0xD0, 0x4B, 0xB7, 0x8B, 0x0F, 0xAC, 0xF9,
            0xB0, 0x91, 0x8B, 0x0F, 0x53, 0xC7, 0x18, 0x16, 0x8B, 0x0F, 0x3E, 0x8F, 0x2E, 0x44,
            0x8B, 0x0F, 0x79, 0x2A, 0xA0, 0x97, 0x3E, 0x0F, 0x5C, 0xDE, 0x6E, 0x7C, 0x8B, 0x0F,
            0x6C, 0x34, 0xE4, 0x4C, 0x8B, 0x0F, 0xB2, 0xEB, 0xB2, 0x57, 0x8B, 0x0F, 0x80, 0x48,
            0x4A, 0x71, 0x8B, 0x0F, 0x40, 0x8A, 0xE7, 0x36, 0x8B, 0x0F, 0x42, 0x00, 0x07, 0x01,
            0x01, 0x00, 0x4A, 0xD0, 0x4B, 0xB7, 0x8C, 0x0F,
        ];
        let srv_list = vec![
            "74.208.75.183:3979",
            "172.249.176.145:3979",
            "83.199.24.22:3979",
            "62.143.46.68:3979",
            "121.42.160.151:3902",
            "92.222.110.124:3979",
            "108.52.228.76:3979",
            "178.235.178.87:3979",
            "128.72.74.113:3979",
            "64.138.231.54:3979",
        ].into_iter()
            .map(|s| SocketAddr::from_str(s).unwrap())
            .collect::<Vec<SocketAddr>>();

        (data, srv_list)
    }

    #[test]
    fn test_p_parse_response() {
        let (data, srv_list) = fixtures();

        assert_eq!(
            parse_data(data)
                .wait()
                .map(Result::unwrap)
                .collect::<Vec<SocketAddr>>(),
            srv_list
        )
    }
}
