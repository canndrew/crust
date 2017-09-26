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

// Defines `Core`, the mio handler and the core of the event loop.

use common::{Result, State};
use maidsafe_utilities::thread::{self, Joiner};
use mio::{Event, Events, Poll, PollOpt, Ready, Token};
use mio::channel::{self, Receiver, Sender};
use mio::timer::{Timeout, Timer};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

pub struct EventLoop {
    shutdown_tx: oneshot::Sender<()>,
    _joiner: Joiner,
    remote: Remote,
}

pub fn spawn_event_loop(
    event_loop_id: Option<&str>,
) -> Result<EventLoop> {
    let mut name = "CRUST-Event-Loop".to_string();
    if let Some(id) = event_loop_id {
        name.push_str(": ");
        name.push_str(id);
    }

    let mut core = Core::new()?;
    let remote = core.remote();
    let (shutdown_tx, shutdown_rx) = futures::sync::oneshot::channel();
    let joiner = thread::named(event_loop_id, move || {
        core.run(shutdown_rx)
    });

    EventLoop {
        shutdown_tx: shutdown_tx,
        joiner: joiner,
        remote: remote,
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        if let Err(e) = self.tx.send(()) {
            warn!(
                "Could not send a terminator to event-loop. We will possibly not be able to \
                   gracefully exit. Error: {:?}",
                e
            );
        }
    }
}

