//! Configuration factories for integrations tests.

pub mod proxy {
    use std::net::SocketAddr;

    pub fn target(addr: SocketAddr) -> rxh::config::Server {
        target_with_uri(addr, "/")
    }

    pub fn target_with_uri(addr: SocketAddr, uri: &str) -> rxh::config::Server {
        rxh::config::Server {
            listen: vec!["127.0.0.1:0".parse().unwrap()],
            patterns: vec![rxh::config::Pattern {
                uri: String::from(uri),
                action: rxh::config::Action::Forward(vec![addr]),
            }],
        }
    }
}
