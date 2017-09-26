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

use common::{Core, Message, Priority, Socket, Uid};
use main::{ConnectionId, ConnectionMap};
use mio::{Poll, PollOpt, Ready, Token};
use std::any::Any;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::mem;
use std::rc::Rc;

pub fn connection_candidate(
    socket: Socket,
    cm: ConnectionMap<UID>,
    our_id: UID,
    their_id: UID,
) -> IoFuture<Option<Socket>> {
    match unwrap!(self.cm.lock()).get(&self.their_id) {
        // NOTE: the way we use this seems to be pretty racey...
        Some(&ConnectionId { active_connection: Some(_), .. }) => {
            return future::ok(None).boxed();
        },
        _ => (),
    };

    if our_id > their_id {
        socket.send((Message::ChooseConnection, 0))
            .map(|socket| Some(socket))
    } else {
        socket.into_future()
            .and_then(|(msg, socket)| {
                match msg {
                    Some(Message::ChooseConnection) => Some(socket),
                    _ => None,
                }
            })
    }
}

