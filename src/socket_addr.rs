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

use std::net;
use std::ops::Deref;
use std::str::FromStr;
use rustc_serialize::{Encodable, Decodable, Encoder, Decoder};

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SocketAddr(net::SocketAddr);

impl Deref for SocketAddr {
    type Target = net::SocketAddr;

    fn deref(&self) -> &net::SocketAddr {
        &self.0
    }
}

impl Encodable for SocketAddr {
    fn encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        let as_string = self.0.to_string();
        s.emit_str(&as_string[..])
    }
}

impl Decodable for SocketAddr {
    fn decode<D: Decoder>(d: &mut D) -> Result<SocketAddr, D::Error> {
        let as_string = try!(d.read_str());
        match SocketAddr::from_str(&as_string[..]) {
            Ok(sa) => Ok(SocketAddr(sa)),
            Err(e) => {
                let err = format!("Failed to decode SocketAddr: {}", e);
                Err(d.error(&err[..]))
            }
        }
    }
}
