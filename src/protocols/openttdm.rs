extern crate std;

extern crate rgs_models as models;

use errors;

use errors::{Error, ErrorKind};
use protocols::models as pmodels;
use util;
use util::*;
use enum_primitive::FromPrimitive;

enum_from_primitive! {
    #[derive(Clone, Debug, PartialEq)]
enum IPVer {
    V4,
    V6,
    AUTODETECT,
}
}

enum_from_primitive! {
    #[derive(Clone, Debug, PartialEq)]
enum PktType {
    PacketUdpClientFindServer,
    PacketUdpServerResponse,
    PacketUdpClientDetailInfo,
    PacketUdpServerDetailInfo,
    PacketUdpServerRegister,
    PacketUdpMasterAckRegister,
    PacketUdpClientGetList,
    PacketUdpMasterResponseList,
    PacketUdpServerUnregister,
    PacketUdpClientGetNewgrfs,
    PacketUdpServerNewgrfs,
    PacketUdpMasterSessionKey,
    PacketUdpEnd,
}
}

fn parse_v4(
    len: u16,
    buf: Box<std::iter::Iterator<Item = u8>>,
) -> errors::Result<Vec<std::net::IpAddr>> {
    unimplemented!()
}

fn parse_v6(
    len: u16,
    buf: Box<std::iter::Iterator<Item = u8>>,
) -> errors::Result<Vec<std::net::IpAddr>> {
    unimplemented!()
}

#[derive(Debug)]
pub struct Protocol {
    config: pmodels::Config,
}

impl Protocol {
    fn parse_data(&self, b: Vec<u8>) -> errors::Result<Vec<std::net::IpAddr>> {
        let mut buf = b.into_iter();

        {
            let t = PktType::from_u8(next_item(&mut buf)?).ok_or_else(|| {
                Error::from(ErrorKind::InvalidPacketError("Unknown packet type".into()))
            })?;

            if t != PktType::PacketUdpMasterResponseList {
                return Err(
                    ErrorKind::InvalidPacketError(format!("Invalid packet type: {:?}", t))
                        .into(),
                );
            }
        }

        let len = util::to_u16(&[next_item(&mut buf)?, next_item(&mut buf)?]);

        match IPVer::from_u8(next_item(&mut buf)?).ok_or(
            Error::from_kind(
                ErrorKind::InvalidPacketError("Unknown IP type".into()),
            ),
        )? {
            IPVer::V4 => parse_v4(len, Box::from(buf)),
            IPVer::V6 => parse_v6(len, Box::from(buf)),
            _ => Err(Error::from(
                ErrorKind::InvalidPacketError("Invalid IP type".into()),
            )),
        }
    }
}

impl pmodels::Protocol for Protocol {
    fn make_request(&self) -> Vec<u8> {
        vec![2, 2]
    }

    fn parse_response(&self, p: &pmodels::Packet) -> errors::Result<pmodels::ParseResult> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::str::FromStr;

    fn fixtures() -> (Vec<u8>, Vec<std::net::SocketAddr>) {
        let data = vec![
            0x42,
            0x00,
            0x07,
            0x01,
            0x0A,
            0x00,
            0x4A,
            0xD0,
            0x4B,
            0xB7,
            0x8B,
            0x0F,
            0xAC,
            0xF9,
            0xB0,
            0x91,
            0x8B,
            0x0F,
            0x53,
            0xC7,
            0x18,
            0x16,
            0x8B,
            0x0F,
            0x3E,
            0x8F,
            0x2E,
            0x44,
            0x8B,
            0x0F,
            0x79,
            0x2A,
            0xA0,
            0x97,
            0x3E,
            0x0F,
            0x5C,
            0xDE,
            0x6E,
            0x7C,
            0x8B,
            0x0F,
            0x6C,
            0x34,
            0xE4,
            0x4C,
            0x8B,
            0x0F,
            0xB2,
            0xEB,
            0xB2,
            0x57,
            0x8B,
            0x0F,
            0x80,
            0x48,
            0x4A,
            0x71,
            0x8B,
            0x0F,
            0x40,
            0x8A,
            0xE7,
            0x36,
            0x8B,
            0x0F,
            0x42,
            0x00,
            0x07,
            0x01,
            0x01,
            0x00,
            0x4A,
            0xD0,
            0x4B,
            0xB7,
            0x8C,
            0x0F,
        ];
        let srv_list = vec![
            std::net::SocketAddr::from_str("74.208.75.183:3979").unwrap(),
        ];

        (data, srv_list)
    }


}
