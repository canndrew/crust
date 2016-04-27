use transport::Endpoint;
use socket_addr::SocketAddr;
use rustc_serialize::{Encodable, Encoder, Decoder};

/// This struct contains information needed to Bootstrap and for echo-server services
#[derive(RustcEncodable, RustcDecodable, Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct StaticContactInfo {
    /// This will contain both local and global addresses. Local addresses will be useful on LAN
    /// when someone wants to bootstrap off us and we haven't yet obtained our global address. In
    /// that case the list will contain only the local addresses that the process calling
    /// seek_peers() will get and use.
    pub endpoints: Vec<Endpoint>,
    /// UDP mapper server addresses
    pub udp_mapper_servers: Vec<SocketAddr>,
    /// TCP mapper server addresses
    pub tcp_mapper_servers: Vec<SocketAddr>,
}

