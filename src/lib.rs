extern crate log;
extern crate rand;
extern crate resolve;
#[macro_use]
extern crate serde_json;
extern crate rgs_models as models;
#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate error_chain;

use std::io::Write;

use protocols::models as pmodels;
use errors::Error;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

#[macro_use]
pub mod errors;
#[macro_use]
pub mod util;
pub mod protocols;

#[derive(Clone, Debug)]
pub struct UserRequest {
    pub protocol: pmodels::TProtocol,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct QueryEntry {
    pub protocol: pmodels::TProtocol,
    pub host: std::net::SocketAddr,
}

#[derive(Clone, Debug)]
pub struct ServerEntry {
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

pub trait QueryService {
    fn query_udp(
        &self,
        req: Vec<UserRequest>,
    ) -> Result<std::collections::HashSet<ServerEntry>, Error>;
}

pub struct RealQueryManager {
    logger: Arc<util::LoggingService + Send + Sync + 'static>,
    output_sink: Arc<Write>,
}

impl RealQueryManager {
    pub fn new<LOG, W>(logger: LOG, output_sink: W) -> RealQueryManager
    where
        LOG: util::LoggingService + Send + Sync + 'static,
        W: Write + Send + Sync + 'static,
    {
        RealQueryManager {
            logger: Arc::new(logger),
            output_sink: Arc::new(output_sink),
        }
    }
}

impl QueryService for RealQueryManager {
    fn query_udp(
        &self,
        req: Vec<UserRequest>,
    ) -> errors::Result<std::collections::HashSet<ServerEntry>> {
        let (srv_tx, srv_rx) = channel();
        self.logger.debug("Created channel");

        let mut dnsconf = resolve::config::default_config()?;
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

        let srv_history = Arc::new(Mutex::new(std::collections::HashMap::<
            std::net::SocketAddr,
            pmodels::TProtocol,
        >::new()));

        let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::new(0, 0, 0, 0), 0)).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::new(5, 0)))
            .unwrap();
        self.logger.debug(&format!(
            "Connected to {}",
            socket.local_addr().unwrap()
        ));

        // Send outgoing requests to servers
        let request_handle = {
            let socket = socket.try_clone().unwrap();
            let srv_history = srv_history.clone();
            std::thread::spawn({
                let logger = Arc::clone(&self.logger);
                move || -> Result<(), Error> {
                    loop {
                        let srv = srv_rx.recv()?;
                        let req = srv.protocol.make_request();
                        logger.debug(&format!("Sending request: {:?}", &req));
                        socket.send_to(req.as_slice(), &srv.host)?;
                        (*srv_history).lock().unwrap().insert(
                            srv.host.clone(),
                            Arc::clone(&srv.protocol),
                        );
                    }
                }
            })
        };

        // Read incoming bytes
        let (data_recv_tx, data_recv_rx) = channel();
        let receive_handle = {
            let socket = socket.try_clone().unwrap();
            std::thread::spawn({
                let logger = Arc::clone(&self.logger);
                move || -> Result<(), Error> {
                    loop {
                        let mut buf = vec![0; 1000000];
                        let (recv_size, addr) = socket.recv_from(buf.as_mut_slice())?;

                        buf.truncate(recv_size);

                        logger.debug(&format!("Received bytes: {:?}", &buf));

                        data_recv_tx.send((addr, buf)).unwrap();
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
                        Some(prot) => {
                            let data = prot.parse_response(&protocols::models::Packet {
                                addr: recv_data.0,
                                data: recv_data.1,
                            }).unwrap();
                            for srv in data.servers {
                                data_collect_tx
                                    .send(ServerEntry::new(prot.clone(), srv))
                                    .unwrap();
                            }
                            for srv in data.follow_up {
                                srv_tx
                                    .send(QueryEntry {
                                        host: srv.0,
                                        protocol: srv.1,
                                    })
                                    .unwrap();
                            }
                        }
                        _ => {}
                    }
                }
            })
        };

        let recv_handle = receive_handle.join().unwrap();
        let rsp_res = response_handle.join().unwrap();
        self.logger.debug(&format!("Response thread complete"));
        let req_res = request_handle.join().unwrap();
        self.logger.debug(&format!("Request thread complete"));

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
}
