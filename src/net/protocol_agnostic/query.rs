use net::protocol_agnostic::{ListenerMsg, ListenerMsgKind};
use priv_prelude::*;
use std::error::Error;
use std::hash::{Hash, Hasher};

#[derive(Debug)]
pub struct PaTcpAddrQuerier {
    addr: SocketAddr,
    server_pk: PublicKeys,
}

impl Hash for PaTcpAddrQuerier {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.addr.hash(state)
    }
}

impl PaTcpAddrQuerier {
    pub fn new(addr: &SocketAddr, server_pk: PublicKeys) -> PaTcpAddrQuerier {
        PaTcpAddrQuerier {
            addr: *addr,
            server_pk,
        }
    }
}

impl TcpAddrQuerier for PaTcpAddrQuerier {
    #[allow(trivial_casts)]
    fn query(&self, bind_addr: &SocketAddr, handle: &Handle) -> BoxFuture<SocketAddr, Box<Error>> {
        let handle = handle.clone();
        let handle0 = handle.clone();
        let server_pk = self.server_pk.clone();
        TcpStream::connect_reusable(bind_addr, &self.addr, &handle)
            .map_err(|e| match e {
                ConnectReusableError::Bind(e) => QueryError::Bind(e),
                ConnectReusableError::Connect(e) => QueryError::Connect(e),
            })
            .and_then(move |stream| {
                let our_sk = SecretKeys::new();
                let our_pk = our_sk.public_keys().clone();
                let msg = ListenerMsg {
                    client_pk: our_pk,
                    kind: ListenerMsgKind::EchoAddr,
                };
                let msg = try_bfut!(
                    server_pk
                        .encrypt_anonymous(&msg)
                        .map_err(QueryError::Encrypt)
                );
                let shared_secret = our_sk.shared_secret(&server_pk);
                Framed::new(stream)
                    .send(msg)
                    .map_err(QueryError::Write)
                    .and_then(move |framed| {
                        framed
                            .into_future()
                            .map_err(|(e, _framed)| QueryError::Read(e))
                            .and_then(move |(msg_opt, _framed)| {
                                let msg = msg_opt.ok_or(QueryError::Disconnected)?;
                                let msg: SocketAddr =
                                    shared_secret.decrypt(&msg).map_err(QueryError::Decrypt)?;
                                Ok(msg)
                            })
                    })
                    .into_boxed()
            })
            .with_timeout(Duration::from_secs(3), &handle0)
            .and_then(|addr_opt| addr_opt.ok_or(QueryError::TimedOut))
            .map_err(|e| Box::new(e) as Box<Error>)
            .into_boxed()
    }
}

#[derive(Debug)]
pub struct PaUdpAddrQuerier {
    addr: SocketAddr,
    server_pk: PublicKeys,
}

impl Hash for PaUdpAddrQuerier {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.addr.hash(state)
    }
}

impl PaUdpAddrQuerier {
    pub fn new(addr: &SocketAddr, server_pk: PublicKeys) -> PaUdpAddrQuerier {
        PaUdpAddrQuerier {
            addr: *addr,
            server_pk,
        }
    }
}

impl UdpAddrQuerier for PaUdpAddrQuerier {
    #[allow(trivial_casts)]
    fn query(&self, bind_addr: &SocketAddr, handle: &Handle) -> BoxFuture<SocketAddr, Box<Error>> {
        let handle = handle.clone();
        let handle0 = handle.clone();
        let (socket, _listener) = try_bfut!(
            UdpSocket::bind_connect_reusable(bind_addr, &self.addr, &handle)
                .and_then(|socket| UtpSocket::from_socket(socket, &handle))
                .map_err(QueryError::Bind)
                .map_err(|e| Box::new(e) as Box<Error>)
        );

        let server_pk = self.server_pk.clone();
        socket
            .connect(&self.addr)
            .map_err(QueryError::Connect)
            .and_then(move |stream| {
                let our_sk = SecretKeys::new();
                let our_pk = our_sk.public_keys().clone();
                let msg = ListenerMsg {
                    client_pk: our_pk,
                    kind: ListenerMsgKind::EchoAddr,
                };
                let msg = try_bfut!(
                    server_pk
                        .encrypt_anonymous(&msg)
                        .map_err(QueryError::Encrypt)
                );
                let shared_secret = our_sk.shared_secret(&server_pk);
                Framed::new(stream)
                    .send(msg)
                    .map_err(QueryError::Write)
                    .and_then(move |framed| {
                        framed
                            .into_future()
                            .map_err(|(e, _framed)| QueryError::Read(e))
                            .and_then(move |(msg_opt, _framed)| {
                                let msg = msg_opt.ok_or(QueryError::Disconnected)?;
                                let msg: SocketAddr =
                                    shared_secret.decrypt(&msg).map_err(QueryError::Decrypt)?;
                                Ok(msg)
                            })
                    })
                    .into_boxed()
            })
            .with_timeout(Duration::from_secs(3), &handle0)
            .and_then(|addr_opt| addr_opt.ok_or(QueryError::TimedOut))
            .map_err(|e| Box::new(e) as Box<Error>)
            .into_boxed()
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum QueryError {
        Bind(e: io::Error) {
            description("error binding to port")
            display("error binding to port: {}", e)
            cause(e)
        }
        Connect(e: io::Error) {
            description("error connecting to remote endpoint")
            display("error connecting to remote endpoint: {}", e)
            cause(e)
        }
        Write(e: io::Error) {
            description("error writing to socket")
            display("error writing to socket: {}", e)
            cause(e)
        }
        Read(e: io::Error) {
            description("error reading from socket")
            display("error reading from socket")
            cause(e)
        }
        Encrypt(e: EncryptionError) {
            description("error encrypting message to send to echo server")
            display("error encrypting message to send to echo server: {}", e)
            cause(e)
        }
        Decrypt(e: EncryptionError) {
            description("error decrypting message from echo server")
            display("error decrypting message from echo server: {}", e)
            cause(e)
        }
        Disconnected {
            description("the peer disconnected without sending a response")
        }
        TimedOut {
            description("timed out waiting for a response from the peer")
        }
    }
}

#[cfg(test)]
mod test {
    use priv_prelude::*;
    use tokio_core::reactor::Core;
    use futures::stream::FuturesUnordered;
    use futures::future::Either;
    use env_logger;

    #[test]
    fn multiple_utp_queriers_in_parallel() {
        const NUM_QUERIERS: u32 = 2;

        let _ = env_logger::init();

        let mut core = unwrap!(Core::new());
        let handle = core.handle();

        core.run(future::lazy(|| {
            let socket = unwrap!(UdpSocket::bind_reusable(&addr!("0.0.0.0:0"), &handle));
            let bind_addr = unwrap!(socket.local_addr()).unspecified_to_localhost();

            let mut futures_unordered = FuturesUnordered::new();
            for _ in 0..NUM_QUERIERS {
                let sk = SecretKeys::new();
                let pk = sk.public_keys().clone();
                let listener = unwrap!(PaListener::bind(&utp_addr!("0.0.0.0:0"), &handle, sk));

                let addr = unwrap!(listener.local_addr()).unspecified_to_localhost();
                let addr = match addr {
                    PaAddr::Utp(addr) => addr,
                    PaAddr::Tcp(..) => panic!("expected utp addr"),
                };
                let incoming = listener.incoming();
                let querier = PaUdpAddrQuerier::new(&addr, pk);

                let query = {
                    querier
                        .query(&bind_addr, &handle)
                        .map_err(|e| panic!("query failed: {}", e))
                };
                let accept = {
                    Timeout::new(Duration::from_secs(1), &handle)
                    .infallible()
                    .and_then(|()| incoming.into_future())
                    .map_err(|(e, _)| panic!("incoming errored: {}", e))
                };
                let select = query.select2(accept);
                let future = select.map(move |either| match either {
                    Either::A((recv_addr, _accept)) => {
                        assert_eq!(recv_addr, bind_addr);
                    },
                    Either::B(..) => panic!("accepted something somehow"),
                }).map_err(|either| {
                    let (v, _) = either.split();
                    v
                });

                futures_unordered.push(future);
            }

            futures_unordered.for_each(|()| Ok(()))
        })).void_unwrap()
    }
}

