use errors;
use errors::Error;
use models;
use protocols::models::*;
use util;
use util::*;

use byteorder::NetworkEndian;
use chrono::prelude::*;
use futures;
use futures::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct V2Data {
    pub max_companies: u8,
    pub current_companies: u8,
    pub max_spectators: u8,
}

#[derive(Clone, Debug)]
pub struct V3Data {
    pub current_time: DateTime<Utc>,
    pub start_time: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct V4Data {
    pub active_newgrf: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub enum ProtocolVer {
    V1,
    V2(V2Data),
    V3(V2Data, V3Data),
    V4(V2Data, V3Data, V4Data),
}

impl<'a> From<&'a ProtocolVer> for u8 {
    fn from(v: &'a ProtocolVer) -> u8 {
        match *v {
            ProtocolVer::V1 => 1,
            ProtocolVer::V2(_) => 2,
            ProtocolVer::V3(_, _) => 3,
            ProtocolVer::V4(_, _, _) => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerInfo {
    pub protocol_ver: ProtocolVer,
    pub name: String,
    pub server_version: String,
    pub language_id: u8,
    pub need_pass: bool,
    pub max_clients: u8,
    pub num_clients: u8,
    pub current_spectators: u8,
    pub map: String,
    pub map_set: u8,
    pub dedicated: bool,
}

fn write_v2_data(srv: &mut Server, v: V2Data) {
    srv.rules
        .insert("max-companies".into(), json!(v.max_companies));
    srv.rules
        .insert("current-companies".into(), json!(v.current_companies));
    srv.rules
        .insert("max-spectators".into(), json!(v.max_spectators));
}

fn write_v3_data(srv: &mut Server, v: V3Data) {
    srv.rules
        .insert("current-time".into(), json!(v.current_time.timestamp()));
    srv.rules
        .insert("start-time".into(), json!(v.start_time.timestamp()));
}

fn write_v4_data(srv: &mut Server, v: V4Data) {
    srv.rules
        .insert("active-newgrf".into(), json!(v.active_newgrf));
}

impl From<(SocketAddr, ServerInfo)> for Server {
    fn from(v: (SocketAddr, ServerInfo)) -> Self {
        let (addr, info) = v;
        let mut srv = Server::new(addr);
        srv.status = Status::Up;

        srv.name = Some(info.name);
        srv.need_pass = Some(info.need_pass);
        srv.map = Some(info.map);
        srv.num_clients = Some(info.num_clients as u64);
        srv.max_clients = Some(info.max_clients as u64);

        srv.rules.insert(
            "protocol-version".into(),
            json!(u8::from(&info.protocol_ver)),
        );

        match info.protocol_ver {
            ProtocolVer::V1 => {}
            ProtocolVer::V2(v2) => {
                write_v2_data(&mut srv, v2);
            }
            ProtocolVer::V3(v2, v3) => {
                write_v2_data(&mut srv, v2);
                write_v3_data(&mut srv, v3);
            }
            ProtocolVer::V4(v2, v3, v4) => {
                write_v2_data(&mut srv, v2);
                write_v3_data(&mut srv, v3);
                write_v4_data(&mut srv, v4);
            }
        }

        srv.rules
            .insert("server-version".into(), json!(info.server_version));
        srv.rules
            .insert("language-id".into(), json!(info.language_id));
        srv.rules
            .insert("current-spectators".into(), json!(info.current_spectators));
        srv.rules.insert("map-set".into(), json!(info.map_set));
        srv.rules.insert("dedicated".into(), json!(info.dedicated));

        srv
    }
}

fn parse_v4_data(mut iter: &mut Iterator<Item = u8>) -> Result<V4Data, Error> {
    let mut v = V4Data::default();
    let active_newgrf_num = next_item(&mut iter)?.clone();
    for _ in 0..active_newgrf_num {
        let mut id = String::new();
        let mut hash = String::new();
        for _ in 0..4 {
            id += &format!("{}", util::hex_str(&next_item(&mut iter)?));
        }
        for _ in 0..16 {
            hash += &format!("{}", util::hex_str(&next_item(&mut iter)?));
        }
        v.active_newgrf.insert(id, hash);
    }
    Ok(v)
}

fn parse_v3_data(mut iter: &mut Iterator<Item = u8>) -> Result<V3Data, Error> {
    let current_time = DateTime::from_utc(
        NaiveDateTime::from_timestamp(
            util::to_u32_dyn::<NetworkEndian>(next_items(&mut iter, 4)?.as_slice()).unwrap() as i64,
            0,
        ),
        Utc,
    );
    let start_time = DateTime::from_utc(
        NaiveDateTime::from_timestamp(
            util::to_u32_dyn::<NetworkEndian>(next_items(&mut iter, 4)?.as_slice()).unwrap() as i64,
            0,
        ),
        Utc,
    );

    Ok(V3Data {
        current_time,
        start_time,
    })
}

fn parse_v2_data(mut iter: &mut Iterator<Item = u8>) -> Result<V2Data, Error> {
    Ok(V2Data {
        max_companies: next_item(&mut iter)?,
        current_companies: next_item(&mut iter)?,
        max_spectators: next_item(&mut iter)?,
    })
}

fn parse_data(buf: Vec<u8>) -> Result<ServerInfo, Error> {
    let mut iter = buf.into_iter();

    for _ in 0..3 {
        next_item(&mut iter)?;
    }

    let protocol_ver = match next_item(&mut iter)? {
        0 | 1 => Ok(ProtocolVer::V1),
        2 => {
            let v2 = parse_v2_data(&mut iter)?;
            Ok(ProtocolVer::V2(v2))
        }
        3 => {
            let v3 = parse_v3_data(&mut iter)?;
            let v2 = parse_v2_data(&mut iter)?;
            Ok(ProtocolVer::V3(v2, v3))
        }
        4 => {
            let v4 = parse_v4_data(&mut iter)?;
            let v3 = parse_v3_data(&mut iter)?;
            let v2 = parse_v2_data(&mut iter)?;
            Ok(ProtocolVer::V4(v2, v3, v4))
        }
        _ => Err(Error::DataParseError {
            reason: "Unsupported protocol version".into(),
        }),
    }?;

    let name = util::read_string(&mut iter, 0)?;
    let server_version = util::read_string(&mut iter, 0)?;

    let language_id = next_item(&mut iter)?;
    let need_pass = next_item(&mut iter)? > 0;
    let max_clients = next_item(&mut iter)?;
    let num_clients = next_item(&mut iter)?;
    let current_spectators = next_item(&mut iter)?;

    if u8::from(&protocol_ver) < 3 {
        for _ in 0..2 {
            next_item(&mut iter)?;
        }
        for _ in 0..2 {
            next_item(&mut iter)?;
        }
    }

    let map = util::read_string(&mut iter, 0)?;
    for _ in 0..2 {
        next_item(&mut iter)?;
    }
    for _ in 0..2 {
        next_item(&mut iter)?;
    }
    let map_set = next_item(&mut iter)?;
    let dedicated = next_item(&mut iter)? > 0;

    Ok(ServerInfo {
        protocol_ver,
        name,
        server_version,
        language_id,
        need_pass,
        max_clients,
        num_clients,
        current_spectators,
        map,
        map_set,
        dedicated,
    })
}

#[derive(Debug)]
pub struct ProtocolImpl;

impl Protocol for ProtocolImpl {
    fn make_request(&self, _state: Option<Value>) -> Vec<u8> {
        vec![3, 0, 0]
    }

    fn parse_response(&self, p: Packet) -> Box<Stream<Item = ParseResult, Error = Error>> {
        let data = p.data;
        let addr = p.addr;
        Box::new(futures::stream::iter_result(vec![parse_data(data).map(
            move |info| ParseResult::Output(Server::from((addr, info))),
        )]))
    }
}

#[cfg(test)]
mod tests {
    extern crate serde_json;

    use super::*;
    use protocols::models::Protocol;
    use std::str::FromStr;
    fn fixtures() -> (SocketAddr, Vec<u8>, Server) {
        let addr = SocketAddr::from_str("127.0.0.1:9000").unwrap();
        let data = vec![
            0x86, 0x00, 0x01, 0x04, 0x03, 0x4D, 0x47, 0x03, 0x05, 0x2E, 0x96, 0xB9, 0xAB, 0x2B,
            0xEA, 0x68, 0x6B, 0xFF, 0x94, 0x96, 0x1A, 0xD4, 0x33, 0xA7, 0x01, 0x32, 0x32, 0x33,
            0x22, 0x31, 0x61, 0x80, 0xDA, 0x1B, 0xA6, 0x44, 0x4A, 0x06, 0xCD, 0x17, 0xF8, 0xFA,
            0x79, 0xD6, 0x0A, 0x44, 0x4E, 0x07, 0x00, 0x48, 0xB3, 0xF9, 0xE4, 0xFD, 0x0D, 0xF2,
            0xA7, 0x2B, 0x5F, 0x44, 0xD3, 0xC8, 0xA2, 0xF4, 0xA0, 0x63, 0xEC, 0x0A, 0x00, 0x63,
            0xEC, 0x0A, 0x00, 0x0F, 0x00, 0x0A, 0x4F, 0x6E, 0x6C, 0x79, 0x46, 0x72, 0x69, 0x65,
            0x6E, 0x64, 0x73, 0x20, 0x4F, 0x70, 0x65, 0x6E, 0x54, 0x54, 0x44, 0x20, 0x53, 0x65,
            0x72, 0x76, 0x65, 0x72, 0x20, 0x23, 0x31, 0x00, 0x31, 0x2E, 0x35, 0x2E, 0x33, 0x00,
            0x16, 0x00, 0x19, 0x00, 0x00, 0x52, 0x61, 0x6E, 0x64, 0x6F, 0x6D, 0x20, 0x4D, 0x61,
            0x70, 0x00, 0x00, 0x04, 0x00, 0x04, 0x01, 0x01,
        ];
        let mut srv = Server::new(addr.clone());
        srv.status = Status::Up;
        srv.name = Some("OnlyFriends OpenTTD Server #1".into());
        srv.map = Some("Random Map".into());
        srv.num_clients = Some(0);
        srv.max_clients = Some(25);
        srv.need_pass = Some(false);
        srv.rules.insert("protocol-version".into(), json!(4));
        srv.rules.insert(
            "active-newgrf".into(),
            json!({
                "4d470305": "2e96b9ab2bea686bff94961ad433a701",
                "32323322": "316180da1ba6444a06cd17f8fa79d60a",
                "444e0700": "48b3f9e4fd0df2a72b5f44d3c8a2f4a0",
            }),
        );
        srv.rules.insert("current-time".into(), 1676413440.into());
        srv.rules.insert("start-time".into(), 1676413440.into());
        srv.rules.insert("max-companies".into(), 15.into());
        srv.rules.insert("current-companies".into(), 0.into());
        srv.rules.insert("server-version".into(), "1.5.3".into());
        srv.rules.insert("language-id".into(), 22.into());
        srv.rules.insert("current-spectators".into(), 0.into());
        srv.rules.insert("max-spectators".into(), 10.into());
        srv.rules.insert("map-set".into(), 1.into());
        srv.rules.insert("dedicated".into(), true.into());

        (addr, data, srv)
    }

    #[test]
    fn test_p_make_request() {
        let fixture = json!({});

        let expectation = vec![3, 0, 0];
        let result = ProtocolImpl.make_request(None);

        assert_eq!(expectation, result);
    }

    #[test]
    fn test_p_parse_response() {
        let p = ProtocolImpl;
        let (addr, data, server) = fixtures();

        let expectation = vec![ParseResult::Output(server)];

        let result = p.parse_response(Packet {
            addr: addr,
            data: data,
        }).wait()
            .map(|res| res.unwrap())
            .collect::<Vec<ParseResult>>();

        assert_eq!(result, expectation);
    }
}
