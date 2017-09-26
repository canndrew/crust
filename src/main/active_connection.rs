// Copyright 2016 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement.  This, along with the Licenses can be
// found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

use common::{Core, CoreTimer, CrustUser, Message, Priority, Socket, State, Uid};
use main::{ConnectionId, ConnectionMap, Event};
use mio::{Poll, Ready, Token};
use mio::timer::Timeout;
use std::any::Any;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::Duration;

#[cfg(not(test))]
pub const INACTIVITY_TIMEOUT_MS: u64 = 120_000;
#[cfg(not(test))]
const HEARTBEAT_PERIOD_MS: u64 = 20_000;

#[cfg(test)]
pub const INACTIVITY_TIMEOUT_MS: u64 = 900;
#[cfg(test)]
const HEARTBEAT_PERIOD_MS: u64 = 300;

pub struct ConnectionHandle<UID: Uid> {
    sender: mpsc::Sender<(Message<UID>, Priority)>,
}

pub struct ActiveConnection<UID: Uid> {
    socket: Socket,
    cm: ConnectionMap<UID>,
    their_id: UID,
    their_role: CrustUser,
    event: Event<UID>,
    event_tx: ::CrustEventSender<UID>,
    receiver: mpsc::Reciever<Message<UID>>,

    heartbeat_recv_timeout: Timeout,
    heartbeat_send_timeout: Timeout,
}

impl<UID: Uid> ActiveConnection<UID> {
    pub fn new(
        socket: Socket,
        cm: ConnectionMap<UID>,
        our_id: UID,
        their_id: UID,
        their_role: CrustUser,
        event: Event<UID>,
        event_tx: ::CrustEventSender<UID>,
    ) -> ActiveConnection<UID> {
        trace!(
            "Entered state ActiveConnection: {:?} -> {:?}",
            our_id,
            their_id
        );

        let (tx, rx) = unbounded::channel();
        {
            let mut guard = unwrap!(cm.lock());
            {
                let conn_id = guard.entry(their_id).or_insert(ConnectionId {
                    active_connection: None,
                    currently_handshaking: 1,
                });
                // TODO: change the way we use this map so that we can guarantee we don't have a race
                // condition.
                conn_id.active_connection = Some(tx);
            }
            trace!(
                "Connection Map inserted: {:?} -> {:?}",
                their_id,
                guard.get(&their_id)
            );
        }

        ActiveConnection {
            socket: socket,
            cm: cm,
            their_id: their_id,
            their_role: their_role,
            event_tx: event_tx,
            receiver: rx,

            heartbeat_recv_timeout: Timeout::new(Duration::from_millis(INACTIVITY_TIMEOUT_MS)),
            heartbeat_send_timeout: Timeout::new(Duration::from_millis(HEARTBEAT_PERIOD_MS)),
        }
    }
}

impl Future for ActiveConnection {
    type Item = ();
    type Error = Void;

    fn poll(&mut self) -> Result<Async<()>, Void> {
        loop {
            match unwrap!(self.receiver.poll()) {
                Async::Ready(Some(x)) => {
                    self.socket.send(x);
                    self.heartbeat_send_timeout.reset(Duration::from_millis(HEARTBEAT_PERIOD_MS));
                },
                Async::Ready(None) => {
                    return Ok(Async::Ready(()));
                },
                Async::NotReady => break,
            }
        }

        while let Async::Ready(msg) = self.socket.poll()? {
            let msg = unwrap!(msg);
            self.heartbeat_recv_timeout.reset(Duration::from_millis(INACTIVITY_TIMEOUT_MS));
            match msg {
                Message::Data(data) => {
                    self.event_tx.send(Event::NewMessage(
                        self.their_id,
                        self.their_role,
                        data,
                    ))?;
                }
                Message::Heartbeat => (),
                message => {
                    debug!("{:?} - Unexpected message: {:?}", self.our_id, message);
                }
            }
        }

        if let Async::Ready(()) = self.heartbeat_recv_timeout.poll() {
            return Ok(Async::Ready(());
        }

        while let Async::Ready(()) = self.heartbeat_send_timeout.poll() {
            socket.send((Message::Heartbeat, 0));
            self.heartbeat_send_timeout.reset(Duration::from_millis(HEARTBEAT_PERIOD_MS));
        }
    }
}

impl<UID: Uid> Drop for ActiveConnection<UID> {
    fn drop(&mut self) {
        {
            let mut guard = unwrap!(self.cm.lock());
            if let Entry::Occupied(mut oe) = guard.entry(self.their_id) {
                oe.get_mut().active_connection = None;
                if oe.get().currently_handshaking == 0 {
                    let _ = oe.remove();
                }
            }
            trace!(
                "Connection Map removed: {:?} -> {:?}",
                self.their_id,
                guard.get(&self.their_id)
            );
        }
        let _ = self.event_tx.send(Event::LostPeer(self.their_id));
    }
}

/*
 * TODO:
    #[cfg(not(test))]
    /// Helper function that returns a socket address of the connection
    pub fn peer_addr(&self) -> ::Res<SocketAddr> {
        use main::CrustError;
        self.socket.peer_addr().map_err(CrustError::Common)
    }

    #[cfg(test)]
    // TODO(nbaksalyar) find a better way to mock connection IPs
    pub fn peer_addr(&self) -> ::Res<SocketAddr> {
        use std::str::FromStr;
        Ok(unwrap!(FromStr::from_str("192.168.0.1:0")))
    }

    pub fn peer_kind(&self) -> CrustUser {
        self.their_role
    }
*/

