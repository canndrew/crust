// Copyright 2015 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement, version 1.0.  This, along with the
// Licenses can be found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};
use std::thread;
use std::str::FromStr;

use beacon;
use bootstrap_handler::BootstrapHandler;
use config_handler::Config;
//use getifaddrs::{getifaddrs, filter_loopback};
use transport;
use transport::{Endpoint, Protocol, Message, Handshake};
use std::thread::JoinHandle;
use std::net::{SocketAddr, UdpSocket, SocketAddrV4, SocketAddrV6, TcpListener};

use itertools::Itertools;
use event::{Event, ContactInfoResult, ContactInfo};
use connection::Connection;
use sequence_number::SequenceNumber;
use hole_punching::HolePunchServer;
use util::{ip_from_socketaddr, SocketAddrW};
use util;

// Closure is a wapper around boxed closures that tries to work around the fact
// that it is not possible to call Box<FnOnce> in the current stable rust.
// The idea is to wrap the FnOnce in something that implements FnMut using some
// dirty tricks, because Box<FnMut> is fine to call.
//
// This workaround can be removed once FnBox becomes stable or Box<FnOnce>
// becomes usable.
pub struct Closure(Box<FnMut(&mut State) + Send>);

impl Closure {
    pub fn new<F : FnOnce(&mut State) + Send + 'static>(f: F) -> Closure {
        let mut f = Some(f);
        Closure(Box::new(move |state: &mut State| {
            if let Some(f) = f.take() {
                f(state)
            }
        }))
    }

    pub fn invoke(mut self, state: &mut State) {
        (self.0)(state)
    }
}

pub struct ConnectionData {
    pub message_sender: Sender<Message>,
    pub mapper_address: Option<SocketAddr>,
    pub mapper_external_address: Option<SocketAddrV4>,
}

pub struct State {
    pub event_sender        : ::CrustEventSender,
    pub cmd_sender          : Sender<Closure>,
    pub cmd_receiver        : Receiver<Closure>,
    pub connections         : HashMap<Connection, ConnectionData>,
    pub listening_endpoints : HashMap<SocketAddr, Vec<SocketAddr>>,
    pub bootstrap_handler   : BootstrapHandler,
    pub stop_called         : bool,
    pub is_bootstrapping    : bool,
    pub next_punch_sequence : SequenceNumber,
    pub mapper              : HolePunchServer,
    pub mapped_sockets      : HashMap<u32, UdpSocket>,
    pub mapped_socket_send  : Sender<(u32, UdpSocket)>,
    pub mapped_socket_recv  : Receiver<(u32, UdpSocket)>,
}

impl State {
    pub fn new(event_sender: ::CrustEventSender) -> io::Result<State> {
        let (cmd_sender, cmd_receiver) = mpsc::channel::<Closure>();
        let mapper = try!(::hole_punching::HolePunchServer::start(cmd_sender.clone()));
        let (send, recv) = mpsc::channel();

        Ok(State {
            event_sender        : event_sender,
            cmd_sender          : cmd_sender,
            cmd_receiver        : cmd_receiver,
            connections         : HashMap::new(),
            listening_endpoints : HashMap::new(),
            bootstrap_handler   : BootstrapHandler::new(),
            stop_called         : false,
            is_bootstrapping    : false,
            next_punch_sequence : SequenceNumber::new(::rand::random()),
            mapper              : mapper,
            mapped_sockets      : HashMap::new(),
            mapped_socket_send  : send,
            mapped_socket_recv  : recv,
        })
    }

    pub fn run(&mut self) {
        let mut state = self;
        loop {
            match state.cmd_receiver.recv() {
                Ok(cmd) => cmd.invoke(&mut state),
                Err(_) => break,
            }
            if state.stop_called {
                break;
            }
        }
    }

    pub fn update_bootstrap_contacts(&mut self,
                                     contacts_to_add: Vec<SocketAddr>,
                                     contacts_to_remove: Vec<SocketAddr>)
    {
        let _ = self.bootstrap_handler.update_contacts(contacts_to_add,
                                                       contacts_to_remove);
    }

    pub fn get_accepting_endpoints(&self) -> Vec<SocketAddr> {
        let mut ret = Vec::new();
        for mapped_addrs in self.listening_endpoints.values() {
            ret.extend(mapped_addrs.iter());
        }
        ret
    }

    pub fn populate_bootstrap_contacts(&mut self,
                                       config: &Config,
                                       beacon_port: Option<u16>,
                                       own_beacon_guid_and_port: &Option<([u8; 16], u16)>)
            -> Vec<SocketAddr> {
        let cached_contacts = self.bootstrap_handler.read_file().unwrap_or(vec![]);

        let beacon_guid = own_beacon_guid_and_port.map(|(guid, _)| guid);

        let beacon_discovery = match beacon_port {
            Some(port) => Self::seek_peers(beacon_guid, port),
            None => vec![]
        };

        let mut combined_contacts
            = beacon_discovery.into_iter()
            .chain(config.hard_coded_contacts.iter().map(|a| a.0.clone()))
            .chain(cached_contacts.into_iter())
            .unique() // Remove duplicates
            .collect::<Vec<_>>();

        // remove own endpoints
        let own_listening_endpoint = self.get_accepting_endpoints();
        combined_contacts.retain(|c| !own_listening_endpoint.contains(&c));
        combined_contacts
    }

    fn handle_handshake(mut handshake : Handshake,
                        mut trans : transport::Transport)
            -> io::Result<(Handshake, transport::Transport)> {
        let handshake_err = Err(io::Error::new(io::ErrorKind::Other,
                                               "handshake failed"));

        handshake.remote_ip = SocketAddrW(trans.connection_id.peer_endpoint().get_address());
        if let Err(_) = trans.sender.send_handshake(handshake) {
            return handshake_err
        }

        trans.receiver.receive_handshake()
                      .and_then(|handshake| Ok((handshake, trans)))
                      .or(handshake_err)
    }

    pub fn send(&mut self, connection: Connection, bytes: Vec<u8>) {
        let writer_channel = match self.connections.get(&connection) {
            Some(connection_data) => connection_data.message_sender.clone(),
            None => {
                // Connection already destroyed or never existed.
                return;
            }
        };

        if let Err(_what) = writer_channel.send(Message::UserBlob(bytes)) {
            self.unregister_connection(connection);
        }
    }

    pub fn connect(handshake : Handshake,
                   remote_ep : Endpoint)
            -> io::Result<(Handshake, transport::Transport)> {
        Self::handle_handshake(handshake,
                               try!(transport::connect(remote_ep)))
    }

    pub fn rendezvous_connect(handshake: Handshake, udp_socket: UdpSocket,
                              public_ep: Endpoint /* of B */)
            -> io::Result<(Handshake, transport::Transport)> {
        Self::handle_handshake(handshake,
                               try!(transport::rendezvous_connect(udp_socket,
                                                                  public_ep)))
    }

    pub fn accept(handshake : Handshake,
                  listener  : &TcpListener)
            -> io::Result<(Handshake, transport::Transport)> {
        Self::handle_handshake(handshake, try!(transport::accept(listener)))
    }

    pub fn handle_connect(&mut self, token: u32, handshake: Handshake,
                          trans: transport::Transport)
                          -> io::Result<Connection> {
        let c = trans.connection_id.clone();
        let our_external_endpoint = match trans.connection_id.peer_endpoint() {
            Endpoint::Utp(..) => Endpoint::Utp(handshake.remote_ip.0),
            Endpoint::Tcp(..) => Endpoint::Tcp(handshake.remote_ip.0),
        };
        let event = Event::OnConnect(Ok((our_external_endpoint, c)), token);

        let connection = self.register_connection(handshake, trans, event);
        if let Ok(ref connection) = connection {
            if connection.transport_protocol == Protocol::Tcp {
                self.update_bootstrap_contacts(vec![connection.peer_addr.0], vec![]);
            }
        }
        connection
    }

    pub fn handle_rendezvous_connect(&mut self,
                                     token: u32,
                                     handshake: Handshake,
                                     trans: transport::Transport)
                                     -> io::Result<Connection> {
        let c = trans.connection_id.clone();
        let our_external_endpoint = match trans.connection_id.peer_endpoint() {
            Endpoint::Utp(..) => Endpoint::Utp(handshake.remote_ip.0),
            Endpoint::Tcp(..) => Endpoint::Tcp(handshake.remote_ip.0),
        };
        let event = Event::OnRendezvousConnect(Ok((our_external_endpoint, c)), token);
        self.register_connection(handshake, trans, event)
    }

    fn register_connection(&mut self,
                           handshake     : Handshake,
                           trans         : transport::Transport,
                           event_to_user : Event) -> io::Result<Connection> {
        let connection = trans.connection_id.clone();

        debug_assert!(!self.connections.contains_key(&connection));
        let (tx, rx) = mpsc::channel();

        let mapper_addr = handshake.mapper_port
            .map(|port| {
                let peer_addr = trans.connection_id.peer_endpoint().get_address();
                match peer_addr {
                    SocketAddr::V4(a) => {
                        SocketAddr::V4(SocketAddrV4::new(*a.ip(), port))
                    },
                    SocketAddr::V6(a) => {
                        SocketAddr::V6(SocketAddrV6::new(*a.ip(), port,
                                                         a.flowinfo(),
                                                         a.scope_id()))
                    },
                }
            });

        let connection_data = ConnectionData {
            message_sender: tx,
            mapper_address: mapper_addr,
            mapper_external_address: handshake.external_ip.map(|sa| sa.0),
        };

        // We need to insert the event into event_sender *before* the
        // reading thread starts. It is because the reading thread
        // also inserts events into the pipe and if done very quickly
        // they may be inserted in wrong order.
        let _ = self.connections.insert(connection, connection_data);
        let _ = self.event_sender.send(event_to_user);

        self.start_writing_thread(trans.sender, connection.clone(), rx);
        self.start_reading_thread(trans.receiver, connection.clone());

        Ok(trans.connection_id)
    }

    // pushing messages out to socket
    fn start_writing_thread(&self, mut sender     : transport::Sender,
                                   connection     : Connection,
                                   writer_channel : Receiver<Message>) {
        let cmd_sender = self.cmd_sender.clone();

        let _ = Self::new_thread("writer", move || {
            for msg in writer_channel.iter() {
                if sender.send(&msg).is_err() {
                    break;
                }
            }
            let _ = cmd_sender.send(Closure::new(move |state : &mut State| {
                state.unregister_connection(connection);
            }));
        });
    }

    // pushing events out to event_sender
    fn start_reading_thread(&self, mut receiver : transport::Receiver,
                            connection : Connection) {
        let cmd_sender = self.cmd_sender.clone();
        let sink       = self.event_sender.clone();

        let _ = Self::new_thread("reader", move || {
            while let Ok(msg) = receiver.receive() {
                match msg {
                    Message::UserBlob(msg) => {
                        if sink.send(Event::NewMessage(connection.clone(), msg)).is_err() {
                            break
                        }
                    },
                    Message::HolePunchAddress(a) => {
                        let _ = cmd_sender.send(Closure::new(move |state: &mut State| {
                            if let Some(cd) = state.connections.get_mut(&connection.clone()) {
                                cd.mapper_external_address = Some(a.0);
                            }
                        }));
                    },
                }
            }
            let _ = cmd_sender.send(Closure::new(move |state : &mut State| {
                state.unregister_connection(connection);
            }));
        });
    }

    pub fn unregister_connection(&mut self, connection: Connection) {
        // Avoid sending duplicate LostConnection event.
        if self.connections.remove(&connection).is_none() {
            return;
        }

        let _ = self.event_sender.send(Event::LostConnection(connection));
    }

    pub fn handle_accept(&mut self,
                         handshake : Handshake,
                         trans     : transport::Transport)
            -> io::Result<Connection> {
        let c = trans.connection_id.clone();
        let our_external_endpoint = match trans.connection_id.peer_endpoint() {
            Endpoint::Utp(..) => Endpoint::Utp(handshake.remote_ip.0),
            Endpoint::Tcp(..) => Endpoint::Tcp(handshake.remote_ip.0),
        };
        self.register_connection(handshake, trans, Event::OnAccept(our_external_endpoint, c))
    }

    fn seek_peers(beacon_guid: Option<[u8; 16]>, beacon_port: u16) -> Vec<SocketAddr> {
        match beacon::seek_peers(beacon_port, beacon_guid) {
            Ok(peers) => peers,
            Err(_) => Vec::new(),
        }
    }

    pub fn bootstrap_off_list(&mut self,
                              token: u32,
                              mut bootstrap_list: Vec<SocketAddr>) {
        if self.is_bootstrapping { return; }
        self.is_bootstrapping = true;

        bootstrap_list.retain(|e| !self.is_connected_to(&Endpoint::Tcp(e.clone())));

        if bootstrap_list.is_empty() {
            let _ = self.event_sender.send(Event::BootstrapFinished);
            return;
        }

        let head = bootstrap_list.remove(0);

        let event_sender = self.event_sender.clone();
        let cmd_sender   = self.cmd_sender.clone();
        let mapper_port  = self.mapper.listening_addr().port();
        let external_ip  = self.mapper.external_address();

        let _ = Self::new_thread("bootstrap_off_list", move || {
            let h = Handshake {
                mapper_port: Some(mapper_port),
                external_ip: external_ip.map(util::SocketAddrV4W),
                remote_ip: SocketAddrW(SocketAddr::from_str("0.0.0.0:0").unwrap()),
            };

            let connect_result = Self::connect(h, Endpoint::Tcp(head.clone()));

            let _ = cmd_sender.send(Closure::new(move |state: &mut State| {
                if !state.is_bootstrapping {
                    let _ = event_sender.send(Event::BootstrapFinished);
                    return;
                }

                state.is_bootstrapping = false;

                if let Ok(c) = connect_result {
                    let _ = state.handle_connect(token, c.0, c.1);
                }

                state.bootstrap_off_list(token, bootstrap_list);
            }));
        });
    }

    pub fn stop_bootstrap(&mut self) {
        self.is_bootstrapping = false;
    }

    pub fn stop(&mut self) {
        self.stop_called = true;
    }

    fn new_thread<F,T>(name: &str, f: F) -> io::Result<JoinHandle<T>>
            where F: FnOnce() -> T, F: Send + 'static, T: Send + 'static {
        thread::Builder::new().name("State::".to_string() + name)
                              .spawn(f)
    }

    fn is_connected_to(&self, endpoint: &Endpoint) -> bool {
        for pair in self.connections.iter() {
            if pair.0.peer_endpoint() == *endpoint {
                return true;
            }
        }
        return false;
    }

    fn get_ordered_helping_nodes(&self) -> Vec<SocketAddr> {
        let mut addrs = self.connections.iter()
                            .filter_map(|pair| pair.1.mapper_address)
                            .collect::<Vec<_>>();

        addrs.sort_by(|&addr1, &addr2| {
            ::util::heuristic_geo_cmp(&ip_from_socketaddr(addr1),
                                      &ip_from_socketaddr(addr2)).reverse()
        });

        addrs
    }

    pub fn prepare_contact_info(&mut self, result_token: u32) {
        use ::hole_punching::blocking_get_mapped_udp_socket;

        let seq_id = self.next_punch_sequence.number();
        self.next_punch_sequence.increment();

        let event_sender = self.event_sender.clone();
        let helping_nodes = self.get_ordered_helping_nodes();
        let static_endpoints = self.get_accepting_endpoints().into_iter()
                                                             .map(|a| SocketAddrW(a))
                                                             .collect();
        let mapped_socket_send = self.mapped_socket_send.clone();
        let _result_handle = Self::new_thread("map_udp", move || {
            let result = blocking_get_mapped_udp_socket(seq_id, helping_nodes);

            let result = match result {
                // TODO (peterj) use _rest
                Ok((socket, opt_mapped_addr, _rest)) => {
                    let addrs = opt_mapped_addr.into_iter().map(|a| SocketAddrW(a)).collect();
                    let _ = mapped_socket_send.send((result_token, socket));
                    Ok(ContactInfo {
                        static_endpoints: static_endpoints,
                        rendezvous_endpoints: addrs,
                    })
                },
                Err(what) => Err(what),
            };

            let _ = event_sender.send(
                Event::OnContactInfo(ContactInfoResult {
                    result_token: result_token,
                    contact_info: result,
                })
            );
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::thread;
    use std::net::{Ipv4Addr, TcpListener, SocketAddr, SocketAddrV4, SocketAddrV6};
    use ip::IpAddr;
    use std::sync::mpsc::channel;
    use transport::{Endpoint, Port, Handshake};
    use event::Event;
    use util;

    fn testable_endpoint(listener: &TcpListener) -> Endpoint {
        let addr = listener.local_addr().uwrap();

        let ip = util::loopback_if_unspecified(util::ip_from_socketaddr(addr));
        let addr = match (ip, addr) {
            (IpAddr::V4(ip), _) => {
                SocketAddr::V4(SocketAddrV4::new(ip, addr.port()))
            },
            (IpAddr::V6(ip), SocketAddr::V6(addr)) => {
                SocketAddr::V6(SocketAddrV6::new(ip, addr.port(),
                                                 addr.flowinfo(),
                                                 addr.scope_id()))
            },
            _ => panic!("Unreachable"),
        };
        Endpoint::Tcp(addr)
    }

    fn test_bootstrap_off_list(n: u16) {
        let acceptors = (0..n).map(|_| TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 0))).unwrap())
                              .collect::<Vec<_>>();

        let eps = acceptors.iter().map(|a|testable_endpoint(&a))
                                  .collect();

        let (category_tx, category_rx) = channel();
        let (event_tx, event_receiver) = channel();
        let crust_event_category = ::maidsafe_utilities::event_sender::MaidSafeEventCategory::CrustEvent;
        let event_sender = ::maidsafe_utilities::event_sender::MaidSafeObserver::new(event_tx,
                                                                                     crust_event_category,
                                                                                     category_tx);

        let mut s = State::new(event_sender).unwrap();

        let cmd_sender = s.cmd_sender.clone();

        cmd_sender.send(Closure::new(move |s: &mut State| {
            s.bootstrap_off_list(0, eps);
        })).unwrap();

        let accept_thread = thread::spawn(move || {
            for a in acceptors {
                let _ = State::accept(Handshake::default(), &a).unwrap();
            }
        });

        let t = thread::spawn(move || { s.run(); });

        let mut accept_count = 0;

        for it in category_rx.iter() {
            match it {
                ::maidsafe_utilities::event_sender::MaidSafeEventCategory::CrustEvent => {
                    if let Ok(event) = event_receiver.try_recv() {
                        match event {
                            Event::OnConnect(_, _) => {
                                accept_count += 1;
                                if accept_count == n {
                                    cmd_sender.send(Closure::new(move |s: &mut State| {
                                        s.stop();
                                    })).unwrap();
                                    break;
                                }
                            },
                            Event::LostConnection(_) => { },
                            Event::BootstrapFinished => { },
                            _ => {
                                panic!("Unexpected event {:?}", event);
                            }
                        }
                    }
                },
                _ => unreachable!("This category should not have been fired - {:?}", it),
            }
        }

        t.join().unwrap();
        accept_thread.join().unwrap();
    }

    #[test]
    fn bootstrap_off_list() {
        test_bootstrap_off_list(1);
        test_bootstrap_off_list(2);
        test_bootstrap_off_list(4);
        test_bootstrap_off_list(8);
    }
}

