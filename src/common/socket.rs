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

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use common::{CommonError, MAX_PAYLOAD_SIZE, MSG_DROP_PRIORITY, Priority, Result};
use maidsafe_utilities::serialisation::{deserialise_from, serialise_into};
use mio::{Evented, Poll, PollOpt, Ready, Token};
use mio::tcp::TcpStream;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Cursor, ErrorKind, Read, Write};
use std::mem;
use std::net::{Shutdown, SocketAddr};
use std::time::Instant;

use priority_queue::priority_channel;

pub struct Socket<T: DeserializeOwned + Serialize> {
    inner: Option<Inner<T>>,
}

struct Inner<T: DeserializeOwned + Serialize> {
    sender: IoSink<T>,
    receiver: IoStream<T>,
}

impl<T> Socket<T>
where
    T: DerializeOwned + Serialize
{
    pub fn connect(addr: &SocketAddr) -> IoBoxFuture<Socket> {
        TcpStream::connect(addr).map(Socket::wrap)
    }

    pub fn wrap(stream: TcpStream) -> Self {
        let (read, write) = stream.split();
        let receiver = FramedRead::new(read).filter_map(|data| {
            deserialise(&data[..]).ok()
        }).set_max_frame_length(MAX_PAYLOAD_SIZE);
        
        let (queue_tx, queue_rx) = priority_channel();
        let bg_task = FramedWrite::new(write).send_all(queue_rx)).and_then(|(s, _)| {
            s.into_inner().shutdown()
        });
        handle.spawn(bg_task);

        let sender = queue_tx.with(|msg| {
            future::ok(unwrap!(serialise(msg)))
        }).boxed();

        Socket {
            inner: Some(Inner {
                sender: sender,
                receiver: receiver,
            }),
        }
    }

    /*
     * TODO: determine where these are used.
    pub fn peer_addr(&self) -> Result<SocketAddr> {
        let inner = self.inner.as_ref().ok_or(CommonError::UninitialisedSocket)?;
        Ok(inner.get_ref().peer_addr()?)
    }

    pub fn take_error(&mut self) -> Result<Option<io::Error>> {
        let inner = self.inner.as_ref().ok_or(CommonError::UninitialisedSocket)?;
        Ok(inner.get_mut_ref().take_error()?)
    }
    */
}

impl Default for Socket {
    fn default() -> Self {
        Socket { inner: None }
    }
}

impl<T> Stream for Socket<T> {
    type Item = T;
    type Error = CommonError;

    fn poll(&mut self) -> Result<Async<Option<T>>> {
        self.receiver.poll()
    }
}

impl<T> Sink for Socket<T> {
    type SinkItem = (T, u8);
    type SinkError = CommonError;

    fn start_send(&mut self, arg: (T, u8)) -> Result<AsyncSink<(T, u8)>>> {
        self.sender.start_send(arg)
    }

    fn poll_complete(&mut self) -> Result<Async<()>> {
        self.sender.poll_complete()
    }
}

