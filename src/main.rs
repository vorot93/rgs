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
    pub protocol: pmodels::TProtocolEntry,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug)]
struct QueryEntry {
    pub protocol: pmodels::TProtocolEntry,
    pub host: std::net::SocketAddr,
}

#[derive(Clone, Debug)]
struct ServerEntry {
    protocol: pmodels::TProtocolEntry,
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

impl Eq for ServerEntry {
}

impl ServerEntry {
    fn new(protocol: pmodels::TProtocolEntry, data: models::Server) -> ServerEntry {
        ServerEntry{protocol: protocol, data: data}
    }

    fn into_inner(self) -> (pmodels::TProtocolEntry, models::Server) {
        (self.protocol, self.data)
    }
}

fn query_udp(req: Vec<UserRequest>) -> Result<std::collections::HashSet<ServerEntry>, Error> {
    let (srv_tx, srv_rx) = channel();
    println!("Created channel");

    let mut dnsconf = try!(resolve::config::default_config().map_err(|err| { Error::NetworkError(format!("DNS error: {}", err)) }));
    dnsconf.timeout = std::time::Duration::new(5, 0);
    let dns = resolve::resolver::DnsResolver::new(dnsconf).unwrap();

    let queries = req.iter().fold(Vec::new(), |mut vec, ref srv| {
        match dns.resolve_host(&srv.host) {
            Ok(mut addr) => {
                vec.push(QueryEntry {
                    protocol: srv.protocol.clone(),
                    host: std::net::SocketAddr::new(addr.next().unwrap(), srv.port.clone()),
                });
            }
            Err(_) => {}
        };
        vec
    });

    for q in queries {
        srv_tx.send(q).unwrap();
    }

    let srv_history =
        Arc::new(Mutex::new(std::collections::HashMap::<std::net::SocketAddr,
                                                        pmodels::TProtocolEntry>::new()));

    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::new(0, 0, 0, 0), 0)).unwrap();
    socket.set_read_timeout(Some(std::time::Duration::new(5, 0))).unwrap();
    println!("Connected to {}", socket.local_addr().unwrap());

    let request_handle;
    {
        let socket = socket.try_clone().unwrap();
        let srv_history = srv_history.clone();
        request_handle = std::thread::spawn(move || -> Result<(), Error> {
            loop {
                let srv = try!(srv_rx.recv());
                let prot = (*srv.protocol).lock().unwrap();
                match prot.data
                    .make_request(&prot.config) {
                    Ok(req) => {
                        println!("Sending request: {:?}", &req);
                        try!(socket.send_to(req.as_slice(), &srv.host));
                        (*srv_history)
                            .lock()
                            .unwrap()
                            .insert(srv.host.clone(), srv.protocol.clone());
                    }
                    Err(err) => {
                        println!("Failed to make request: {:?}", err);
                    }
                }
            }
        });
    }

    let receive_handle;
    let (data_recv_tx, data_recv_rx) = channel();
    {
        let socket = socket.try_clone().unwrap();
        receive_handle = std::thread::spawn(move || -> Result<(), Error> {
            loop {
                let mut buf = [0; 65535];
                let (_, addr) = try!(socket.recv_from(&mut buf));

                let data = buf.to_vec();
                println!("Received response: {:?}", data);

                try!(data_recv_tx.send((addr, data)));
            }
        });
    }

    let (data_collect_tx, data_collect_rx) = channel();
    let response_handle;
    {
        let srv_history = srv_history.clone();
        response_handle = std::thread::spawn(move || -> Result<(), Error> {
            loop {
                let recv_data = try!(data_recv_rx.recv());
                match (*srv_history).lock().unwrap().get(&recv_data.0) {
                    Some(p) => {
                        let prot = p.lock().unwrap();
                        let data = prot.data
                            .parse_response(&protocols::models::Packet {
                                                addr: recv_data.0,
                                                data: recv_data.1,
                                            },
                                            &protocols::models::Config::default())
                            .unwrap();
                        for srv in data.0 {
                            data_collect_tx.send(ServerEntry::new(p.clone(), srv)).unwrap();
                        }
                        for srv in data.1 {
                            try!(srv_tx.send(QueryEntry {
                                protocol: match srv.0 {
                                    pmodels::ProtocolRelationship::Current => p.clone(),
                                    pmodels::ProtocolRelationship::Child => match prot.child.as_ref() {
                                        Some(child) => child.clone(),
                                        None => { continue; },
                                    }
                                },
                                host: srv.1,
                            }));
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    let recv_handle = receive_handle.join().unwrap();
    let rsp_res = response_handle.join().unwrap();
    println!("Response thread complete");
    let req_res = request_handle.join().unwrap();
    println!("Request thread complete");

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

    let mut pconfig = pmodels::ProtocolConfig::new();

    {
        let server_p = pmodels::ProtocolConfigEntry::make::<protocols::openttds::P>();
        server_p.lock()
            .unwrap()
            .config
            .insert("prelude-finisher".into(), Value::String("\x00\x00".into()));
        pconfig.insert("openttds".into(), server_p);
    }

    let requests = vec![UserRequest {
            protocol: pconfig.get("openttds".into()).unwrap().clone(),
            host: "ttd.duck.me.uk".into(),
            port: 3979,
        }];

    let data = query_udp(requests);
    println!("{:?}", data);
}
