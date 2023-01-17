//! Configuration factories for integrations tests.

pub mod proxy {
    use std::net::SocketAddr;

    pub fn target(addr: SocketAddr) -> rxh::config::Server {
        target_with_prefix(addr, "/")
    }

    pub fn target_with_prefix(addr: SocketAddr, prefix: &str) -> rxh::config::Server {
        rxh::config::Server {
            listen: "127.0.0.1:0".parse().unwrap(),
            prefix: String::from(prefix),
            kind: rxh::config::Kind::Proxy(rxh::config::Proxy { target: addr }),
        }
    }
}
