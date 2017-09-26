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

use common::{Core, Message, Priority, Socket, State, Uid};
use mio::{Poll, PollOpt, Ready, Token};
use mio::tcp::TcpStream;
use nat::{NatError, util};
use std::any::Any;
use std::cell::RefCell;
use std::net::SocketAddr;
use std::rc::Rc;

pub fn get_ext_addr<UID: Uid>(
    handle: &Handle,
    local_addr: &SocketAddr,
    peer_stun: &SocketAddr,
) -> IoFuture<SocketAddr> {
    let query_socket = util::new_reusably_bound_tcp_socket(&local_addr)?;
    TcpStream::connect_stream(query_socket, peer_stun, handle)
        .and_then(|tcp_socket| {
            let socket = Socket::wrap(tcp_socket);
            socket.send((Message::EchoAddrReq, 0))
        })
        .and_then(|socket| {
            socket.into_future()
        })
        .map(|(msg, _)| {
            if let Some(Message::EchoAddrResp(ext_addr) = msg {
                Ok(ext_addr)
            } else {
                Err(io::ErrorKind::InvalidData.into())
            }
        })
        .boxed()
}

