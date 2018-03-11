use errors::Error;
use protocols::models as pmodels;
use pmodels::{Packet, ParseResult};

use futures;
use futures::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

fn parse_rulestring(data: String) -> Result<HashMap<String, String>, Error> {
    let mut out = HashMap::<String, String>::default();
    let mut split_iter = data.split('\\');

    if let Some(v) = split_iter.next() {
        if v.len() > 0 {
            return Err(Error::DataParseError {
                reason: "First item in split should be empty".to_string(),
            });
        }

        loop {
            match split_iter.next() {
                None => {
                    break;
                }
                Some(k) => {
                    let v = split_iter.next().ok_or(Error::DataParseError {
                        reason: "Early EOL while parsing rule string".into(),
                    })?;

                    out.insert(k.into(), v.into());
                }
            }
        }
    }

    Ok(out)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Rule {
    Secure,
    MaxClients,
    GameType,
    ModName,
    TerrainName,
    NeedPass,
    ServerName,
}

#[derive(Debug)]
pub struct Protocol {
    pub protocol_ver: u8,
    pub default_request_port: u8,
    pub rule_names: HashMap<Rule, String>,
}

impl pmodels::Protocol for Protocol {
    fn make_request(&self, state: Option<Value>) -> Vec<u8> {
        let mut out = vec![255, 255, 255, 255];
        out.append(&mut Vec::from("getstatus RGS".as_bytes()));

        out
    }

    fn parse_response(&self, p: Packet) -> Box<Stream<Item = ParseResult, Error = Error>> {
        Box::new(futures::stream::iter_ok(vec![]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_parse_rulestring() {
        let fixture =
            "\\voip\\opus\\g_needpass\\0\\pure\\1\\gametype\\0\\sv_maxclients\\8".to_string();
        let mut expectation = HashMap::<String, String>::default();
        for &(k, v) in [
            ("voip", "opus"),
            ("g_needpass", "0"),
            ("pure", "1"),
            ("gametype", "0"),
            ("sv_maxclients", "8"),
        ].iter()
        {
            expectation.insert(k.to_string(), v.to_string());
        }

        let result = parse_rulestring(fixture).unwrap();

        assert_eq!(expectation, result);
    }
}
