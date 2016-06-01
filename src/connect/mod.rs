// Copyright 2016 MaidSafe.net limited.
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

mod connection_candidate;
mod establish_direct_connection;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use core::Context;
use peer_id::PeerId;

pub use self::connection_candidate::ConnectionCandidate;
pub use self::establish_direct_connection::EstablishDirectConnection;

pub type ConnectionMap = HashMap<PeerId, Context>;
pub type SharedConnectionMap = Arc<Mutex<ConnectionMap>>;