extern crate std;

extern crate rgs_models as models;

use errors::Error;
use protocols::helpers;
use protocols::models as pmodels;
use std::str::FromStr;
use util;
use enum_primitive::FromPrimitive;

use std::sync::{Arc, Mutex};

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
    PACKET_UDP_CLIENT_FIND_SERVER,
    PACKET_UDP_SERVER_RESPONSE,
    PACKET_UDP_CLIENT_DETAIL_INFO,
    PACKET_UDP_SERVER_DETAIL_INFO,
    PACKET_UDP_SERVER_REGISTER,
    PACKET_UDP_MASTER_ACK_REGISTER,
    PACKET_UDP_CLIENT_GET_LIST,
    PACKET_UDP_MASTER_RESPONSE_LIST,
    PACKET_UDP_SERVER_UNREGISTER,
    PACKET_UDP_CLIENT_GET_NEWGRFS,
    PACKET_UDP_SERVER_NEWGRFS,
    PACKET_UDP_MASTER_SESSION_KEY,
    PACKET_UDP_END,
}
}

pub fn make_request(c: &pmodels::Config) -> Result<Vec<u8>, Error> {
    Ok(vec![2, 2])
}

fn parse_v4(len: u16, buf: std::vec::IntoIter<u8>) -> Result<Vec<std::net::IpAddr>, Error> {
    unimplemented!()
}

fn parse_v6(len: u16, buf: std::vec::IntoIter<u8>) -> Result<Vec<std::net::IpAddr>, Error> {
    unimplemented!()
}

fn parse_data(b: &Vec<u8>) -> Result<Vec<std::net::IpAddr>, Error> {
    let mut buf = b.clone().into_iter();

    {
        let t = PktType::from_u8(try_next!(buf)).ok_or(
            Error::InvalidPacketError(
                format!(
                    "Unknown packet type"
                ),
            ),
        )?;

        if t != PktType::PACKET_UDP_MASTER_RESPONSE_LIST {
            return Err(Error::InvalidPacketError(
                format!("Invalid packet type: {:?}", t),
            ));
        }
    }

    let len = util::to_u16(&[try_next!(buf), try_next!(buf)]);

    Ok(match IPVer::from_u8(try_next!(buf)).ok_or(
        Error::InvalidPacketError(format!("Unknown IP type")),
    )? {
        IPVer::V4 => parse_v4(len, buf),
        IPVer::V6 => parse_v6(len, buf),
        _ => Err(Error::InvalidPacketError(format!("Invalid IP type"))),
    }?)
}

pub fn parse_response(
    p: &pmodels::Packet,
    c: &pmodels::Config,
    us: Arc<Mutex<pmodels::Protocol>>,
    child: Option<Arc<Mutex<pmodels::Protocol>>>,
) -> Result<(Vec<models::Server>, Vec<(Arc<Mutex<pmodels::Protocol>>, std::net::SocketAddr)>), Error> {
    unimplemented!()
}


#[cfg(test)]
mod tests {
    use super::*;

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
