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

use self::get_ext_addr::GetExtAddr;
use common::{Core, CoreMessage, CoreTimer, State, Uid};
use igd::PortMappingProtocol;
use maidsafe_utilities::thread;
use mio::{Poll, Token};
use mio::timer::Timeout;
use nat::{MappingContext, NatError, util};
use net2::TcpBuilder;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::rc::Rc;
use std::time::Duration;

mod get_ext_addr;

const TIMEOUT_SEC: u64 = 3;

pub fn map_tcp_socket<UID: Uid>(
    port: u16,
    mc: &MappingContext
) -> IoFuture<(TcpBuilder, Vec<SocketAddr>)> {

    // TODO(Spandan) Ipv6 is not supported in Listener so dealing only with ipv4 right now
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
    let socket = util::new_reusably_bound_tcp_socket(&addr)?;
    let addr = socket.local_addr()?;

    let mut mappers = Vec::new();
    for &(ref ip, ref gateway) in mc.ifv4s() {
        let gateway = match *gateway {
            Some(ref gateway) => gateway.clone(),
            None => continue,
        };
        let (tx, rx) = oneshot::channel();
        let _ = thread::named("IGD-Address-Mapping", move || {
            let ret = gateway.get_any_address(PortMappingProtocol::TCP, addr_igd, 0, "MaidSafeNat");
            tx.send(ret);
        });
        mappers.push(rx.flatten());
    }

    for stun in mc.peer_stuns() {
        mappers.push(get_ext_addr::get_ext_addr(handle, addr, stun));
    }

    let mut mapped_addrs = mc.ifv4s()
        .iter()
        .map(|&(ip, _)| SocketAddr::new(IpAddr::V4(ip), addr.port()))
        .collect();

    stream::futures_unordered(mappers)
        .timeout_at(Instant::now() + Duration::new(TIMEOUT_SEC))
        .collect()
        .map(|addrs| {
            mapped_addrs.extend(addrs);
            (socket, mapped_addrs)
        })
}

