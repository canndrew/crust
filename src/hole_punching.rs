#[derive(Debug, RustcEncodable, RustcDecodable)]
pub struct HolePunch {
    pub secret: [u8; 4],
    pub ack: bool,
}
