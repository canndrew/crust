// Copyright 2017 MaidSafe.net limited.
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

use futures::Future;
use std::time::Duration;
use tokio_core::reactor::Handle;

use util::with_timeout::WithTimeout;

pub trait FutureExt: Future {
    fn with_timeout(
        self,
        handle: &Handle,
        duration: Duration,
        error: Self::Error,
    ) -> WithTimeout<Self>
    where
        Self: Sized,
    {
        WithTimeout::new(handle, self, duration, error)
    }

    /*
    fn with_timeout_at(self, handle: &Handle, at: Instant, error: Self::Error) -> WithTimeout<Self>
    where
        Self: Sized
    {
        WithTimeout::new_at(handle, self, at, error)
    }
    */
}

impl<F> FutureExt for F
where
    F: Future,
{
}