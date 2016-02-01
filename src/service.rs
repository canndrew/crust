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

use std::io;
use std::sync::{Arc, Mutex};
use service_discovery::ServiceDiscovery;
use sodiumoxide;
use sodiumoxide::crypto::sign;

use connection::RaiiTcpAcceptor;
use udp_listener::UdpListener;
use contact_info::ContactInfo;
use rand;
use endpoint::{Endpoint, Protocol};
use error::Error;
use ip::SocketAddrExt;
use connection;
use bootstrap::RaiiBootstrap;

use event::{Event, OurContactInfo, TheirContactInfo, ContactInfoResult};
use socket_addr::SocketAddr;

/// A structure representing a connection manager.
///
/// This abstraction has a hidden dependency on a config file. Refer to [the docs for `FileHandler`]
/// (../file_handler/struct.FileHandler.html) and [an example config file flowchart]
/// (https://github.com/maidsafe/crust/blob/master/docs/vault_config_file_flowchart.pdf) for more
/// information.
pub struct Service {
    our_contact_info: Arc<Mutex<ContactInfo>>,
    service_discovery: ServiceDiscovery<ContactInfo>,
    _udp_listener: UdpListener,
    event_tx: ::CrustEventSender,
    external_udp_sock_seq_id: u32,
    bootstrap: RaiiBootstrap,
    _raii_tcp_acceptor: RaiiTcpAcceptor,
}

impl Service {
    /// Constructs a service. User needs to create an asynchronous channel, and provide
    /// the sender half to this method. Receiver will receive all `Event`s from this library.
    pub fn new(event_tx: ::CrustEventSender,
               service_discovery_port: u16)
               -> Result<Service, Error> {
        sodiumoxide::init();

        // TODO Use private key once crate is stable
        let (pub_key, _priv_key) = sign::gen_keypair();

        // Form our initial contact info
        let our_contact_info = Arc::new(Mutex::new(ContactInfo {
            pub_key: pub_key,
            tcp_acceptors: Vec::new(),
            udp_listeners: Vec::new(),
        }));

        // Form initial peer contact infos - these will also contain echo-service addrs.
        let peer_contact_infos = Arc::new(Mutex::new(Vec::new()));

        // Start the TCP Acceptor
        let raii_tcp_acceptor = try!(connection::start_tcp_accept(0,
                                                                  our_contact_info.clone(),
                                                                  event_tx.clone()));
        // Start the UDP Listener
        let udp_listener = try!(UdpListener::new(our_contact_info.clone(),
                                                 peer_contact_infos.clone(),
                                                 event_tx.clone()));

        let cloned_contact_info = our_contact_info.clone();
        let generator = move || unwrap_result!(cloned_contact_info.lock()).clone();
        let service_discovery = try!(ServiceDiscovery::new_with_generator(service_discovery_port,
                                                                          generator));

        let bootstrap = RaiiBootstrap::new(&service_discovery,
                                           our_contact_info.clone(),
                                           peer_contact_infos.clone(),
                                           event_tx.clone());

        let service = Service {
            our_contact_info: our_contact_info,
            //peer_contact_infos: peer_contact_infos,
            service_discovery: service_discovery,
            _udp_listener: udp_listener,
            event_tx: event_tx,
            external_udp_sock_seq_id: 0,
            bootstrap: bootstrap,
            _raii_tcp_acceptor: raii_tcp_acceptor,
        };

        Ok(service)
    }

    /// Stop the bootstraping procedure
    pub fn stop_bootstrap(&mut self) {
        self.bootstrap.stop();
    }

    /// Enable or Disable listening to peers trying to find us. The return value indicates
    /// successful registration of the request.
    pub fn set_listen_for_peers(&self, listen: bool) -> bool {
        self.service_discovery.set_listen_for_peers(listen)
    }

    /// Get the hole punch servers addresses of nodes that we're connected to ordered by how likely
    /// they are to be on a seperate network.
    pub fn get_ordered_helping_nodes(&self, protocol: Protocol) -> Vec<SocketAddr> {
        // TODO(canndrew): Whenever me make or accept a connection we should be adding the peers
        // static contact info to a cache (ie. the bootstrap cache). This function should read the
        // cache and choose some socket addresses that might be able to act as rendezvous servers.
        let _ = protocol;
        unimplemented!()
    }

    /// Opens a connection to a remote peer. `public_endpoint` is the endpoint
    /// of the remote peer. `udp_socket` is a socket whose public address will
    /// be used by the other peer.
    ///
    /// A rendezvous connection setup is different to the traditional BSD socket
    /// setup in which there is no client or server side. Both ends create a
    /// socket and send somehow its public address to the other peer. Once both
    /// ends know each other address, both must call this function passing the
    /// socket which possess the address used by the other peer and passing the
    /// other peer's address.
    ///
    /// Only UDP-based protocols are supported. This means that you must use a
    /// uTP endpoint or nothing will happen.
    ///
    /// On success `Event::OnConnect` with connected `Endpoint` will
    /// be sent to the event channel. On failure, nothing is reported. Failed
    /// attempts are not notified back up to the caller. If the caller wants to
    /// know of a failed attempt, it must maintain a record of the attempt
    /// itself which times out if a corresponding
    /// `Event::OnConnect` isn't received. See also [Process for
    /// Connecting]
    /// (https://github.com/maidsafe/crust/blob/master/docs/connect.md) for
    /// details on handling of connect in different protocols.
    pub fn connect(&self, our_contact_info: OurContactInfo, their_contact_info: TheirContactInfo) {
        if let Some(msg) = if our_contact_info.secret != their_contact_info.secret {
            Some("Cannot connect. our_contact_info and their_contact_info are not associated with \
                  the same connection.")
        } else if their_contact_info.udp_rendezvous_addrs.is_empty() {
            Some("No udp rendezvous address supplied. Direct connections and tcp rendezvous connect not yet supported.")
        } else {
            None
        } {
            let err = io::Error::new(io::ErrorKind::Other, msg);
            let ev = Event::NewConnection {
                connection: Err(err),
                their_pub_key: their_contact_info.pub_key,
            };
            let _ = self.event_tx.send(ev);
            return;
        }

        let event_tx = self.event_tx.clone();
        //let our_pub_key = unwrap_result!(self.our_contact_info.lock()).pub_key.clone();

        // TODO(canndrew): blocking_udp_punch_hole should take an array of rendezvous addresses
        // and hole punch to all of them simultaneously.
        // TODO(canndrew): We should also attempt a tcp rendezvous connect here in parallel
        // TODO(canndrew): We should try to connect directly using their static addresses before
        // initiating a rendezvous connect.
        let _joiner = thread!("PeerConnectionThread", move || {
            let (udp_socket, result_addr) =
                ::utp_connections::blocking_udp_punch_hole(our_contact_info.udp_rendezvous_socket,
                                                           our_contact_info.secret,
                                                           their_contact_info.udp_rendezvous_addrs[0]
                                                               .clone());
            let public_endpoint = match result_addr {
                Ok(addr) => addr,
                Err(e) => {
                    let ev = Event::NewConnection {
                        connection: Err(e),
                        their_pub_key: their_contact_info.pub_key,
                    };
                    let _ = event_tx.send(ev);
                    return;
                }
            };

            let _ = event_tx.send(Event::NewConnection {
                connection: connection::utp_rendezvous_connect(udp_socket,
                                                               public_endpoint,
                                                               their_contact_info.pub_key,
                                                               event_tx.clone()),
                their_pub_key: their_contact_info.pub_key,
            });
        });
    }

    /// Get already known external endpoints without any upnp mapping
    pub fn get_known_external_endpoints(&self) -> Vec<Endpoint> {
        unwrap_result!(self.our_contact_info.lock())
            .tcp_acceptors
            .iter()
            .map(|sa| Endpoint::from_socket_addr(Protocol::Tcp, *sa))
            .collect::<Vec<Endpoint>>()
    }

    /// Lookup a mapped udp socket based on result_token
    pub fn prepare_contact_info(&mut self, result_token: u32) {
        use utp_connections::external_udp_socket;
        use tcp_connections::external_tcp_addr;

        let utp_helping_nodes = self.get_ordered_helping_nodes(Protocol::Utp);
        let tcp_helping_nodes = self.get_ordered_helping_nodes(Protocol::Tcp);
        let event_tx = self.event_tx.clone();

        let static_addrs = self.get_known_external_endpoints();
        let our_pub_key = unwrap_result!(self.our_contact_info.lock()).pub_key;

        let seq_id = self.external_udp_sock_seq_id;
        self.external_udp_sock_seq_id += 1;

        let _result_handle = thread!("map sockets", move || {
            let result = external_udp_socket(seq_id, utp_helping_nodes);

            let res = match result {
                // TODO (peterj) use _rest
                Ok((udp_rendezvous_socket, external_udp_addr)) => {
                    match external_tcp_addr(seq_id, tcp_helping_nodes) {
                        Ok((local_tcp_addr, external_tcp_addrs)) => {
                            Ok(OurContactInfo {
                                udp_rendezvous_socket: udp_rendezvous_socket,
                                tcp_rendezvous_local_addr: local_tcp_addr,
                                secret: Some(rand::random()),
                                static_addrs: static_addrs,
                                udp_rendezvous_addrs: vec![external_udp_addr],
                                tcp_rendezvous_addrs: external_tcp_addrs,
                                pub_key: our_pub_key,
                            })
                        },
                        Err(e) => Err(e)
                    }
                }
                Err(what) => Err(what),
            };

            let _ = event_tx.send(Event::ContactInfoPrepared(ContactInfoResult {
                result_token: result_token,
                result: res,
            }));
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use event::Event;

    use std::sync::mpsc;
    use std::sync::mpsc::Receiver;

    use maidsafe_utilities::event_sender::{MaidSafeObserver, MaidSafeEventCategory};

    fn get_event_sender()
        -> (::CrustEventSender,
            Receiver<MaidSafeEventCategory>,
            Receiver<Event>)
    {
        let (category_tx, category_rx) = mpsc::channel();
        let event_category = MaidSafeEventCategory::CrustEvent;
        let (event_tx, event_rx) = mpsc::channel();

        (MaidSafeObserver::new(event_tx, event_category, category_tx),
         category_rx,
         event_rx)
    }

    #[test]
    fn start_stop_service() {
        let (event_sender, _, _) = get_event_sender();
        let _service = unwrap_result!(Service::new(event_sender, 44444));
    }

    #[test]
    fn start_two_services_tcp_connect() {
        let (event_sender_0, category_rx_0, event_rx_0) = get_event_sender();
        let (event_sender_1, category_rx_1, event_rx_1) = get_event_sender();

        let service_0 = unwrap_result!(Service::new(event_sender_0, 5483));
        // let service_0 finish bootstrap - since it is the zero state, it should not find any peer
        // to bootstrap
        {
            let event_rxd = unwrap_result!(event_rx_0.recv());
            match event_rxd {
                Event::BootstrapFinished => (),
                _ => panic!("Received unexpected event: {:?}", event_rxd),
            }
        }
        assert!(service_0.set_listen_for_peers(true));

        let service_1 = unwrap_result!(Service::new(event_sender_1, 5483));
        // let service_1 finish bootstrap - it should bootstrap off service_0
        let (mut connection_1_to_0, pub_key_0) = {
            let event_rxd = unwrap_result!(event_rx_1.recv());
            match event_rxd {
                Event::NewConnection { connection: Ok(connection_obj), their_pub_key } => {
                    (connection_obj, their_pub_key)
                }
                _ => panic!("Received unexpected event: {:?}", event_rxd),
            }
        };

        // now service_1 should get BootstrapFinished
        {
            let event_rxd = unwrap_result!(event_rx_1.recv());
            match event_rxd {
                Event::BootstrapFinished => (),
                _ => panic!("Received unexpected event: {:?}", event_rxd),
            }
        }

        // service_0 should have received service_1's connection bootstrap connection by now
        let (mut connection_0_to_1, pub_key_1) = match unwrap_result!(event_rx_0.recv()) {
            Event::NewConnection { connection: Ok(connection_obj), their_pub_key } => {
                (connection_obj, their_pub_key)
            }
            _ => panic!("0 Should have got a new connection from 1."),
        };

        assert!(pub_key_0 != pub_key_1);

        // send data from 0 to 1
        {
            let data_txd = vec![0, 1, 255, 254, 222, 1];
            unwrap_result!(connection_0_to_1.send(&data_txd));

            // 1 should rx data
            let (data_rxd, peer_pub_key) = {
                let event_rxd = unwrap_result!(event_rx_1.recv());
                match event_rxd {
                    Event::NewMessage(their_pub_key, msg) => (msg, their_pub_key),
                    _ => panic!("Received unexpected event: {:?}", event_rxd),
                }
            };

            assert_eq!(data_rxd, data_txd);
            assert_eq!(peer_pub_key, pub_key_0);
        }

        // send data from 1 to 0
        {
            let data_txd = vec![10, 11, 155, 214, 202];
            unwrap_result!(connection_1_to_0.send(&data_txd));

            // 0 should rx data
            let (data_rxd, peer_pub_key) = {
                let event_rxd = unwrap_result!(event_rx_0.recv());
                match event_rxd {
                    Event::NewMessage(their_pub_key, msg) => (msg, their_pub_key),
                    _ => panic!("Received unexpected event: {:?}", event_rxd),
                }
            };

            assert_eq!(data_rxd, data_txd);
            assert_eq!(peer_pub_key, pub_key_1);
        }
    }

    #[test]
    fn start_two_services_utp_rendezvous_connect() {
        // Start 2 services and get their OurContactInfos. Filter the contact infos to contain just
        // utp rendezvous endpoints and ensure that the two services can connect and exchange
        // messages in both directions.
    }
}
