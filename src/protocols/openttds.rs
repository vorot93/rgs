extern crate std;

extern crate rgs_models as models;

use errors::Error;
use protocols::helpers;
use protocols::models as pmodels;
use util;

fn parse_data(buf: &Vec<u8>, addr: std::net::SocketAddr) -> Result<models::Server, Error> {
    let mut e = models::Server::new(addr);

    let mut iter = buf.iter();

    try_next!(iter);
    try_next!(iter);
    try_next!(iter);

    let protocol_ver = *try_next!(iter);
    e.rules.insert("protocol-version".into(), json!(protocol_ver));
    if protocol_ver >= 4 {
        let active_newgrf_num = try_next!(iter).clone();
        let active_newgrfs = {
            let mut v = Vec::new();
            for _ in 0..active_newgrf_num {
                let mut id = String::new();
                let mut hash = String::new();
                for _ in 0..4 {
                    id += &format!("{}", util::hex_str(try_next!(iter)));
                }
                for _ in 0..16 {
                    hash += &format!("{}", util::hex_str(try_next!(iter)));
                }
                v.push(json!({"id": id, "md5": hash}));
            }
            v
        };
        e.rules.insert("active-newgrfs-num".into(), json!(active_newgrf_num));
        e.rules.insert("active-newgrfs".into(), json!(active_newgrfs));
    }

    if protocol_ver >= 3 {
        e.rules.insert("time-current".into(), json!(util::to_u32(&[try_next!(iter).clone(), try_next!(iter).clone(), try_next!(iter).clone(), try_next!(iter).clone()])));
        e.rules.insert("time-start".into(), json!(util::to_u32(&[try_next!(iter).clone(), try_next!(iter).clone(), try_next!(iter).clone(), try_next!(iter).clone()])));
    }

    if protocol_ver >= 2 {
        e.rules.insert("max-companies".into(), json!(try_next!(iter)));
        e.rules.insert("current-companies".into(), json!(try_next!(iter)));
        e.rules.insert("max-spectators".into(), json!(try_next!(iter)));
    }
    e.name = Some(try!(util::read_string(&mut iter, 0)));
    e.rules.insert("server-version".into(),
                   json!(try!(util::read_string(&mut iter, 0))));

    e.rules.insert("language-id".into(), json!(try_next!(iter)));
    e.need_pass = Some(*try_next!(iter) > 0);
    e.max_clients = Some(try_next!(iter).clone().into());
    e.num_clients = Some(try_next!(iter).clone().into());
    e.rules.insert("current-spectators".into(), json!(try_next!(iter)));
    if protocol_ver < 3 {
        for _ in 0..2 {
            try_next!(iter);
        }
        for _ in 0..2 {
            try_next!(iter);
        }
    }

    e.terrain = Some(try!(util::read_string(&mut iter, 0)));
    for _ in 0..2 {
        try_next!(iter);
    }
    for _ in 0..2 {
        try_next!(iter);
    }
    e.rules.insert("map-set".into(), json!(try_next!(iter)));
    e.rules.insert("dedicated".into(), json!(try_next!(iter)));

    Ok(e)
}

#[derive(Default, Debug)]
pub struct P {}

impl pmodels::Protocol for P {
    fn make_request(&self, c: &pmodels::Config) -> Result<Vec<u8>, Error> {
        macro_rules! default { () => { "{{prelude-starter}}\x03{{prelude-finisher}}" } }
        let t = {
            match c.get("request-template") {
                None => default!(),
                Some(v) => {
                    match v.as_str() {
                        None => default!(),
                        Some(v) => v,
                    }
                }
            }
        };

        helpers::make_request_packet(t, c)
    }

    fn parse_response<'a>(&self,
                          p: &pmodels::Packet,
                          _: &pmodels::Config)
                          -> Result<(Vec<models::Server>,
                                     Vec<(pmodels::ProtocolRelationship, std::net::SocketAddr)>),
                                    Error> {
        let mut v = try!(parse_data(&p.data, p.addr.clone()));
        v.addr = p.addr.into();
        Ok((vec![v], vec![]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use self::pmodels::Protocol;
    fn fixtures() -> (std::net::SocketAddr, Vec<u8>, models::Server) {
        let addr = std::net::SocketAddr::from_str("127.0.0.1:9000").unwrap();
        let data = vec![0x86, 0x00, 0x01, 0x04, 0x03, 0x4D, 0x47, 0x03, 0x05, 0x2E, 0x96, 0xB9, 0xAB, 0x2B, 0xEA, 0x68, 0x6B, 0xFF, 0x94, 0x96, 0x1A, 0xD4, 0x33, 0xA7, 0x01, 0x32, 0x32, 0x33, 0x22, 0x31, 0x61, 0x80, 0xDA, 0x1B, 0xA6, 0x44, 0x4A, 0x06, 0xCD, 0x17, 0xF8, 0xFA, 0x79, 0xD6, 0x0A, 0x44, 0x4E, 0x07, 0x00, 0x48, 0xB3, 0xF9, 0xE4, 0xFD, 0x0D, 0xF2, 0xA7, 0x2B, 0x5F, 0x44, 0xD3, 0xC8, 0xA2, 0xF4, 0xA0, 0x63, 0xEC, 0x0A, 0x00, 0x63, 0xEC, 0x0A, 0x00, 0x0F, 0x00, 0x0A, 0x4F, 0x6E, 0x6C, 0x79, 0x46, 0x72, 0x69, 0x65, 0x6E, 0x64, 0x73, 0x20, 0x4F, 0x70, 0x65, 0x6E, 0x54, 0x54, 0x44, 0x20, 0x53, 0x65, 0x72, 0x76, 0x65, 0x72, 0x20, 0x23, 0x31, 0x00, 0x31, 0x2E, 0x35, 0x2E, 0x33, 0x00, 0x16, 0x00, 0x19, 0x00, 0x00, 0x52, 0x61, 0x6E, 0x64, 0x6F, 0x6D, 0x20, 0x4D, 0x61, 0x70, 0x00, 0x00, 0x04, 0x00, 0x04, 0x01, 0x01];
        let mut srv = models::Server::new(addr.clone());
        srv.name = Some("OnlyFriends OpenTTD Server #1".into());
        srv.terrain = Some("Random Map".into());
        srv.num_clients = Some(0);
        srv.max_clients = Some(25);
        srv.need_pass = Some(false);
        srv.rules.insert("protocol-version".into(), json!(4));
        srv.rules.insert("active-newgrfs-num".into(), json!(3));
        srv.rules.insert("active-newgrfs".into(),
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
        ]));
        srv.rules.insert("time-current".into(), json!(1676413440));
        srv.rules.insert("time-start".into(), json!(1676413440));
        srv.rules.insert("max-companies".into(), json!(15));
        srv.rules.insert("current-companies".into(), json!(0));
        srv.rules.insert("server-version".into(), json!("1.5.3"));
        srv.rules.insert("language-id".into(), json!(22));
        srv.rules.insert("current-spectators".into(), json!(0));
        srv.rules.insert("max-spectators".into(), json!(10));
        srv.rules.insert("map-set".into(), json!(1));
        srv.rules.insert("dedicated".into(), json!(1));

        (addr, data, srv)
    }

    #[test]
    fn test_parse() {
        let (addr, data, expectation) = fixtures();

        let result = parse_data(&data, addr).unwrap();
        assert_eq!(result, expectation);
    }

    #[test]
    fn test_p_make_request() {
        let fixture = json!({
            "prelude-finisher": "\x00\x00",
        });

        let expectation = vec![3, 0, 0];
        let result = P::default().make_request(fixture.as_object().unwrap()).unwrap();

        assert_eq!(expectation, result);
    }

    #[test]
    fn test_p_parse_response() {
        let p = P::default();
        let (addr, data, server) = fixtures();

        let expectation = vec![server];

        let (result, _) = p.parse_response(&pmodels::Packet {
                                addr: addr,
                                data: data,
                            },
                            &pmodels::Config::default())
            .unwrap();

        assert_eq!(result, expectation);
    }
}
