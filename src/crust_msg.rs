use sodiumoxide::crypto::box_::PublicKey;
use socket_addr::SocketAddr;

#[derive(Clone, PartialEq, Eq, Debug, RustcEncodable, RustcDecodable)]
pub enum CrustMsg {
    BootstrapRequest(PublicKey),
    BootstrapResponse(PublicKey),
    ExternalEndpointRequest,
    ExternalEndpointResponse(SocketAddr),
    Connect(PublicKey),
    Message(Vec<u8>),
}
