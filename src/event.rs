use std::io;

use peer_id::PeerId;
use service::ConnectionInfoResult;

/// Enum representing different events that will be sent over the asynchronous channel to the user
/// of this module.
#[derive(Debug)]
pub enum Event {
    /// Invoked when a new message is received.  Passes the message.
    NewMessage(PeerId, Vec<u8>),
    /// Invoked when we get a bootstrap connection to a new peer.
    BootstrapConnect(PeerId),
    /// Invoked when a bootstrap peer connects to us
    BootstrapAccept(PeerId),
    /// Invoked when a connection to a new peer is established.
    NewPeer(io::Result<()>, PeerId),
    /// Invoked when a peer is lost.
    LostPeer(PeerId),
    /// Raised once the list of bootstrap contacts is exhausted.
    BootstrapFinished,
    /// Invoked as a result to the call of `Service::prepare_contact_info`.
    ConnectionInfoPrepared(ConnectionInfoResult),
}

