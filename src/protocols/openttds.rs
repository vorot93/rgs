extern crate futures_await as futures;
extern crate rgs_models as models;
extern crate serde_json;
extern crate std;

use errors;
use errors::Error;
use futures::prelude::*;
use util;
use protocols::helpers;
use protocols::models as pmodels;
use serde_json::Value;
use util::*;

fn parse_data(buf: Vec<u8>, addr: std::net::SocketAddr) -> errors::Result<models::Server> {
    let mut e = models::Server::new(addr);
    e.status = models::Status::Up;

    let mut iter = buf.into_iter();

    for _ in 0..3 {
        next_item(&mut iter)?;
    }

    let protocol_ver = next_item(&mut iter)?;
    e.rules
        .insert("protocol-version".into(), protocol_ver.into());
    if protocol_ver >= 4 {
        let active_newgrf_num = next_item(&mut iter)?.clone();
        let active_newgrfs = {
            let mut v = Vec::new();
            for _ in 0..active_newgrf_num {
                let mut id = String::new();
                let mut hash = String::new();
                for _ in 0..4 {
                    id += &format!("{}", util::hex_str(&next_item(&mut iter)?));
                }
                for _ in 0..16 {
                    hash += &format!("{}", util::hex_str(&next_item(&mut iter)?));
                }
                v.push(json!({"id": id, "md5": hash}));
            }
            v
        };
        e.rules
            .insert("active-newgrfs-num".into(), json!(active_newgrf_num));
        e.rules
            .insert("active-newgrfs".into(), json!(active_newgrfs));
    }

    if protocol_ver >= 3 {
        e.rules.insert(
            "time-current".into(),
            json!(util::to_u32_dyn(next_items(&mut iter, 4)?.as_slice()).unwrap()),
        );
        e.rules.insert(
            "time-start".into(),
            json!(util::to_u32_dyn(next_items(&mut iter, 4)?.as_slice()).unwrap()),
        );
    }

    if protocol_ver >= 2 {
        e.rules
            .insert("max-companies".into(), json!(next_item(&mut iter)?));
        e.rules
            .insert("current-companies".into(), json!(next_item(&mut iter)?));
        e.rules
            .insert("max-spectators".into(), json!(next_item(&mut iter)?));
    }
    e.name = Some(util::read_string(&mut iter, 0)?);
    e.rules.insert(
        "server-version".into(),
        json!(try!(util::read_string(&mut iter, 0))),
    );

    e.rules
        .insert("language-id".into(), json!(next_item(&mut iter)?));
    e.need_pass = Some(next_item(&mut iter)? > 0);
    e.max_clients = Some(next_item(&mut iter)?.into());
    e.num_clients = Some(next_item(&mut iter)?.into());
    e.rules
        .insert("current-spectators".into(), json!(next_item(&mut iter)?));
    if protocol_ver < 3 {
        for _ in 0..2 {
            next_item(&mut iter)?;
        }
        for _ in 0..2 {
            next_item(&mut iter)?;
        }
    }

    e.terrain = Some(util::read_string(&mut iter, 0)?);
    for _ in 0..2 {
        next_item(&mut iter)?;
    }
    for _ in 0..2 {
        next_item(&mut iter)?;
    }
    e.rules
        .insert("map-set".into(), json!(next_item(&mut iter)?));
    e.rules
        .insert("dedicated".into(), json!(next_item(&mut iter)?));

    Ok(e)
}

#[derive(Clone, Debug)]
pub struct Protocol {
    request_template: Vec<u8>,
}

impl Protocol {
    pub fn new(c: &pmodels::Config) -> errors::Result<Protocol> {
        Ok(Self {
            request_template: helpers::make_request_packet(
                c.get("request-template")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{{prelude-starter}}\x03{{prelude-finisher}}".into()),
                c,
            )?,
        })
    }
}

impl pmodels::Protocol for Protocol {
    fn make_request(&self, state: Option<Value>) -> Vec<u8> {
        self.request_template.clone()
    }

    fn parse_response(
        &self,
        p: pmodels::Packet,
    ) -> Box<Stream<Item = pmodels::ParseResult, Error = Error>> {
        let mut v = parse_data(p.data, p.addr.clone())?;
        v.addr = p.addr.into();
        Box::new(futures::stream::iter_ok(pmodels::ParseResult::Output(v)))
    }
}

#[cfg(test)]
mod tests {
    extern crate serde_json;

    use super::*;
    use super::Protocol as IProtocol;
    use protocols::models::Protocol;
    use std::str::FromStr;
    fn fixtures() -> (std::net::SocketAddr, Vec<u8>, models::Server) {
        let addr = std::net::SocketAddr::from_str("127.0.0.1:9000").unwrap();
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
        let mut srv = models::Server::new(addr.clone());
        srv.status = models::Status::Up;
        srv.name = Some("OnlyFriends OpenTTD Server #1".into());
        srv.terrain = Some("Random Map".into());
        srv.num_clients = Some(0);
        srv.max_clients = Some(25);
        srv.need_pass = Some(false);
        srv.rules.insert("protocol-version".into(), json!(4));
        srv.rules.insert("active-newgrfs-num".into(), json!(3));
        srv.rules.insert(
            "active-newgrfs".into(),
            json!([
            {
                "id": "4d470305",
                "md5": "2e96b9ab2bea686bff94961ad433a701",
            },
            {
                "id": "32323322",
                "md5": "316180da1ba6444a06cd17f8fa79d60a",
            },
            {
                "id": "444e0700",
                "md5": "48b3f9e4fd0df2a72b5f44d3c8a2f4a0",
            }
        ]),
        );
        srv.rules.insert("time-current".into(), 1676413440.into());
        srv.rules.insert("time-start".into(), 1676413440.into());
        srv.rules.insert("max-companies".into(), 15.into());
        srv.rules.insert("current-companies".into(), 0.into());
        srv.rules.insert("server-version".into(), "1.5.3".into());
        srv.rules.insert("language-id".into(), 22.into());
        srv.rules.insert("current-spectators".into(), 0.into());
        srv.rules.insert("max-spectators".into(), 10.into());
        srv.rules.insert("map-set".into(), 1.into());
        srv.rules.insert("dedicated".into(), 1.into());

        (addr, data, srv)
    }

    #[test]
    fn test_p_make_request() {
        let fixture = json!({});

        let expectation = vec![3, 0, 0];
        let result = IProtocol::new(&fixture.as_object().unwrap())
            .unwrap()
            .make_request();

        assert_eq!(expectation, result);
    }

    #[test]
    fn test_p_parse_response() {
        let p = IProtocol::new(&Default::default()).unwrap();
        let (addr, data, server) = fixtures();

        let expectation = pmodels::ParseResult {
            servers: vec![server],
            follow_up: vec![],
        };

        let (result, _) = p.parse_response(&pmodels::Packet {
            addr: addr,
            data: data,
        }).unwrap();

        assert_eq!(result, expectation);
    }
}
