extern crate rand;
extern crate resolve;
#[macro_use]
extern crate serde_json;
extern crate rgs_models as models;

use rand::distributions::Sample;
use std::fmt::Debug;
use std::str::FromStr;

use protocols::models as pmodels;
use errors::Error;
use serde_json::Value;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

#[macro_use]
mod util;
mod errors;
mod protocols;

#[derive(Clone, Debug)]
struct UserRequest {
    pub protocol: pmodels::TProtocol,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug)]
struct QueryEntry {
    pub protocol: pmodels::TProtocol,
    pub host: std::net::SocketAddr,
}

#[derive(Clone, Debug)]
struct ServerEntry {
    protocol: pmodels::TProtocol,
    data: models::Server,
}

impl std::hash::Hash for ServerEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.addr.hash(state)
    }
}

impl PartialEq for ServerEntry {
    fn eq(&self, other: &ServerEntry) -> bool {
        self.data.addr == other.data.addr
    }
}

impl Eq for ServerEntry {}

impl ServerEntry {
    fn new(protocol: pmodels::TProtocol, data: models::Server) -> ServerEntry {
        ServerEntry {
            protocol: protocol,
            data: data,
        }
    }

    fn into_inner(self) -> (pmodels::TProtocol, models::Server) {
        (self.protocol, self.data)
    }
}

fn query_udp_full(req: Vec<UserRequest>,
                  log_fn: Arc<Fn(String) + Send + Sync>)
                  -> Result<std::collections::HashSet<ServerEntry>, Error> {
    let (srv_tx, srv_rx) = channel();
    log_fn(format!("Created channel"));

    let mut dnsconf = resolve::config::default_config()
        .map_err(|err| Error::NetworkError(format!("DNS error: {}", err)))?;
    dnsconf.timeout = std::time::Duration::new(5, 0);
    let dns = resolve::resolver::DnsResolver::new(dnsconf).unwrap();

    let queries = req.iter()
        .fold(Vec::new(), |mut vec, ref srv| {
            match dns.resolve_host(&srv.host) {
                Ok(mut addr) => {
                    vec.push(QueryEntry {
                                 protocol: srv.protocol.clone(),
                                 host: std::net::SocketAddr::new(addr.next().unwrap(),
                                                                 srv.port.clone()),
                             });
                }
                Err(_) => {}
            };
            vec
        });

    for q in queries {
        srv_tx.send(q).unwrap();
    }

    let srv_history = Arc::new(Mutex::new(std::collections::HashMap::<std::net::SocketAddr,
                                                                      pmodels::TProtocol>::new()));

    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::new(0, 0, 0, 0), 0)).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::new(5, 0)))
        .unwrap();
    log_fn(format!("Connected to {}", socket.local_addr().unwrap()));

    // Send outgoing requests to servers
    let request_handle = {
        let socket = socket.try_clone().unwrap();
        let srv_history = srv_history.clone();
        std::thread::spawn({
                               let log_fn = log_fn.clone();
                               move || -> Result<(), Error> {
                loop {
                    let srv = srv_rx.recv()?;
                    let prot = (*srv.protocol).lock().unwrap();
                    match (prot.make_request_fn)(&prot.config) {
                        Ok(req) => {
                            log_fn(format!("Sending request: {:?}", &req));
                            socket.send_to(req.as_slice(), &srv.host)?;
                            (*srv_history)
                                .lock()
                                .unwrap()
                                .insert(srv.host.clone(), srv.protocol.clone());
                        }
                        Err(err) => {
                            log_fn(format!("Failed to make request: {:?}", err));
                        }
                    }
                }
            }
                           })
    };

    // Read incoming bytes
    let (data_recv_tx, data_recv_rx) = channel();
    let receive_handle = {
        let socket = socket.try_clone().unwrap();
        std::thread::spawn({
                               let log_fn = log_fn.clone();
                               move || -> Result<(), Error> {
                loop {
                    let mut buf = vec![0; 1000000];
                    let (recv_size, addr) = socket.recv_from(buf.as_mut_slice())?;

                    buf.truncate(recv_size);

                    log_fn(format!("Received bytes: {:?}", &buf));

                    data_recv_tx.send((addr, buf))?;
                }
            }
                           })
    };

    // Parse incoming bytes and send further requests
    let (data_collect_tx, data_collect_rx) = channel();
    let response_handle = {
        let srv_history = srv_history.clone();
        std::thread::spawn(move || -> Result<(), Error> {
            loop {
                let recv_data = try!(data_recv_rx.recv());
                match (*srv_history).lock().unwrap().get(&recv_data.0) {
                    Some(p) => {
                        let prot = p.lock().unwrap();
                        let data = (prot.parse_response_fn)(&protocols::models::Packet {
                                                                 addr: recv_data.0,
                                                                 data: recv_data.1,
                                                             },
                                                            &protocols::models::Config::default(),
                                                            p.clone(),
                                                            prot.child.clone())
                                .unwrap();
                        for srv in data.0 {
                            data_collect_tx
                                .send(ServerEntry::new(p.clone(), srv))
                                .unwrap();
                        }
                        for srv in data.1 {
                            try!(srv_tx.send(QueryEntry {
                                                 protocol: srv.0,
                                                 host: srv.1,
                                             }));
                        }
                    }
                    _ => {}
                }
            }
        })
    };

    let recv_handle = receive_handle.join().unwrap();
    let rsp_res = response_handle.join().unwrap();
    log_fn(format!("Response thread complete"));
    let req_res = request_handle.join().unwrap();
    log_fn(format!("Request thread complete"));

    let mut result = std::collections::HashSet::new();
    loop {
        let srv_res = data_collect_rx.recv();
        if srv_res.is_err() {
            break;
        }
        result.insert(srv_res.unwrap());
    }

    Ok(result)
}


fn main() {
    // let server = ("master.openttd.org", 3978);
    // let p = protocols::openttdm::P::default();
    let log_fn = Arc::new(|msg| {
                              println!("{}", msg);
                          });
    let mut pconfig = pmodels::ProtocolConfig::new();

    {
        let server_p = Arc::new(Mutex::new(pmodels::Protocol {
                                               child: None,
                                               config: {
                                                   let mut m = pmodels::Config::default();
                                                   m.insert("prelude-finisher".into(),
                                                            Value::String("\x00\x00".into()));
                                                   m
                                               },
                                               make_request_fn: protocols::openttds::make_request,
                                               parse_response_fn:
                                                   protocols::openttds::parse_response,
                                           }));
        pconfig.insert("openttds".into(), server_p);
    }

    let requests = vec![UserRequest {
                            protocol: pconfig.get("openttds".into()).unwrap().clone(),
                            host: "ttd.duck.me.uk".into(),
                            port: 3979,
                        }];

    let data = query_udp_full(requests, log_fn.clone());
    log_fn(format!("{:?}", data));
}
