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

use std::sync::{Arc, Mutex};
use std::sync::atomic::{Ordering, AtomicBool};
use std::collections::HashMap;
use std::io;
use std::io::Write;

use maidsafe_utilities::serialisation::deserialise_from;
use transport::{Stream, OutStream, StreamInfo};
use maidsafe_utilities::serialisation::serialise;

use crust_msg::CrustMsg;
use peer_id::PeerId;
use event::Event;
use service::maybe_drop_connection;

pub struct Connection {
    out_stream: OutStream,
    stop_flag: Arc<AtomicBool>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

impl Connection {
    pub fn from_stream(stream: Stream,
                       id: PeerId,
                       connection_map: Arc<Mutex<HashMap<PeerId, Connection>>>,
                       event_tx: ::CrustEventSender) -> Connection
    {
        let (out_stream, mut in_stream) = stream.split();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_cloned = stop_flag.clone();
        let _ = thread!("Connection::from_stream", move || {
            let stop_flag = stop_flag_cloned;
            loop {
                let res = deserialise_from(&mut in_stream);
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                match res {
                    Ok(CrustMsg::Message(msg)) => {
                        let _ = event_tx.send(Event::NewMessage(id, msg));
                    },
                    Ok(m) => {
                        error!("Unexpected message from peer {}: {:?}", id, m);
                    },
                    Err(e) => {
                        error!("Failed to deserialise message from peer {}: {}", id, e);
                        break;
                    }
                }
            }
            maybe_drop_connection(connection_map, id, event_tx);
        });
        Connection {
            out_stream: out_stream,
            stop_flag: stop_flag,
        }
    }

    pub fn send(&mut self, msg: CrustMsg) -> io::Result<()> {
        let msg = unwrap_result!(serialise(&msg));
        match self.out_stream.write_all(&msg) {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = format!("Lost connection to peer: {}", e);
                Err(io::Error::new(io::ErrorKind::Other, msg))
            },
        }
    }

    pub fn info(&self) -> StreamInfo {
        self.out_stream.info()
    }
}

