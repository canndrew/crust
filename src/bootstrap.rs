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

use std::collections::{hash_map, HashMap};
use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, Duration};
use std::thread;
use std::io::Write;

use maidsafe_utilities::thread::RaiiThreadJoiner;
use maidsafe_utilities::serialisation::{serialise, deserialise_from};
use rand;
use rand::Rng;
use service_discovery::ServiceDiscovery;
use sodiumoxide::crypto::box_::PublicKey;
use transport::Stream;
use itertools::Itertools;

use error::Error;
use config::Config;
use connection::Connection;
use static_contact_info::StaticContactInfo;
use event::Event;
use bootstrap_handler::BootstrapHandler;
use peer_id;
use peer_id::PeerId;
use crust_msg::CrustMsg;

const MAX_CONTACTS_EXPECTED: usize = 1500;

pub struct RaiiBootstrap {
    stop_flag: Arc<AtomicBool>,
    _raii_joiner: RaiiThreadJoiner,
}

impl RaiiBootstrap {
    pub fn new(bootstrap_contacts: Vec<StaticContactInfo>,
               our_public_key: PublicKey,
               event_tx: ::CrustEventSender,
               connection_map: Arc<Mutex<HashMap<PeerId, Connection>>>,
               bootstrap_cache: Arc<Mutex<BootstrapHandler>>)
               -> RaiiBootstrap {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let cloned_stop_flag = stop_flag.clone();

        let raii_joiner = RaiiThreadJoiner::new(thread!("RaiiBootstrap", move || {
            RaiiBootstrap::bootstrap(bootstrap_contacts,
                                     our_public_key,
                                     cloned_stop_flag,
                                     event_tx,
                                     connection_map,
                                     bootstrap_cache)
        }));

        RaiiBootstrap {
            stop_flag: stop_flag,
            _raii_joiner: raii_joiner,
        }
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    fn bootstrap(mut bootstrap_contacts: Vec<StaticContactInfo>,
                 our_public_key: PublicKey,
                 stop_flag: Arc<AtomicBool>,
                 event_tx: ::CrustEventSender,
                 connection_map: Arc<Mutex<HashMap<PeerId, Connection>>>,
                 bootstrap_cache: Arc<Mutex<BootstrapHandler>>) {
        rand::thread_rng().shuffle(&mut bootstrap_contacts[..]);
        let mut deadline = Instant::now();
        for contact in bootstrap_contacts {
            deadline = deadline + Duration::from_secs(3);
            // Bootstrapping got cancelled.
            // Later check the bootstrap contacts in the background to see if they are still valid

            if stop_flag.load(Ordering::SeqCst) {
                break;
            }

            println!("Connecting to peer with endpoints: {:?}", &contact.endpoints[..]);
            let res = Stream::direct_connect(&contact.endpoints[..], deadline);
            let mut stream =  match res {
                Ok(stream) => stream,
                Err(e) => {
                    warn!("Error connecting to bootstrap peer: {}", e);
                    continue;
                },
            };
            let request = CrustMsg::BootstrapRequest(our_public_key);
            let request = unwrap_result!(serialise(&request));
            match stream.write_all(&request) {
                Ok(()) => (),
                Err(e) => {
                    warn!("Error writing to stream during bootstrapping: {}", e);
                    continue;
                },
            };
            let result: CrustMsg = match deserialise_from(&mut stream) {
                Ok(msg) => msg,
                Err(e) => {
                    warn!("Error reading from stream during bootstrapping: {}", e);
                    continue;
                },
            };
            let their_id = match result {
                CrustMsg::BootstrapResponse(key) => peer_id::new_id(key),
                msg => {
                    warn!("Unexpected message from peer during bootstrap: {:?}", msg);
                    continue;
                },
            };
            let connection = Connection::from_stream(stream, their_id, connection_map.clone(), event_tx.clone());
            {
                let mut cm = unwrap_result!(connection_map.lock());
                match cm.entry(their_id) {
                    hash_map::Entry::Occupied(..) => {
                        continue;
                    },
                    hash_map::Entry::Vacant(ve) => {
                        let _ = event_tx.send(Event::BootstrapConnect(their_id));
                        let _ = ve.insert(connection);
                    },
                }
            }
            {
                let mut bootstrap_cache = unwrap_result!(bootstrap_cache.lock());
                match bootstrap_cache.update_contacts(vec![], vec![contact]) {
                    Ok(()) => (),
                    Err(e) => {
                        warn!("Unable to update bootstrap cache: {}", e);
                    },
                };
            }
        }

        let _ = event_tx.send(Event::BootstrapFinished);
    }
}

impl Drop for RaiiBootstrap {
    fn drop(&mut self) {
        self.stop();
    }
}

// Returns the peers from service discovery, cache and config for bootstrapping (not to be held)
pub fn get_known_contacts(service_discovery: &ServiceDiscovery<StaticContactInfo>,
                          bootstrap_cache: Arc<Mutex<BootstrapHandler>>,
                          config: &Config)
                          -> Result<Vec<StaticContactInfo>, Error> {
    let (seek_peers_tx, seek_peers_rx) = mpsc::channel();
    if service_discovery.register_seek_peer_observer(seek_peers_tx) {
        let _ = service_discovery.seek_peers();
    }

    let mut contacts = Vec::with_capacity(MAX_CONTACTS_EXPECTED);

    // Get contacts from bootstrap cache
    println!("Reading bootstrap cache");
    contacts.extend(try!(unwrap_result!(bootstrap_cache.lock()).read_file()));
    println!("Read bootstrap cache");

    // Get further contacts from config file - contains seed nodes
    contacts.extend(config.hard_coded_contacts.iter().cloned());

    // Get contacts from service discovery. Give a sec or so for seek peers to find someone on LAN.
    thread::sleep(Duration::from_secs(1));
    while let Ok(static_contact_info) = seek_peers_rx.try_recv() {
        contacts.push(static_contact_info);
    }

    // Please don't be tempted to use a HashSet here because we want to preserve the
    // order in which we collect the contacts
    contacts = contacts.into_iter().unique().collect();


    Ok((contacts))
}

