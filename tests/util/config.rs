//! Configuration factories for integrations tests.

pub mod proxy {
    //! Proxy specific configurations.

    use std::net::SocketAddr;

    use rxh::{
        config::{Action, Algorithm, Backend, Forward, Pattern, Scheduler, Server},
        sched::WeightedRoundRobin,
    };

    /// Forwards all request to a single backend server.
    pub fn single_backend(address: SocketAddr) -> Server {
        single_backend_with_uri(address, "/")
    }

    /// Forwards all requests to a single backend server when the request URI
    /// matches the given URI.
    pub fn single_backend_with_uri(address: SocketAddr, uri: &str) -> Server {
        let backends = vec![Backend { address, weight: 1 }];

        multiple_weighted_backends_with_uri(backends, uri)
    }

    /// Forwards requests to multiple backend servers using the default
    /// algorithm (WRR).
    pub fn multiple_weighted_backends(backends: Vec<Backend>) -> Server {
        multiple_weighted_backends_with_uri(backends, "/")
    }

    /// Forwards requests to multiple backends using WRR for load balancing
    /// only when the request URI matches the given `uri`.
    pub fn multiple_weighted_backends_with_uri(backends: Vec<Backend>, uri: &str) -> Server {
        let scheduler = Scheduler::Wrr(WeightedRoundRobin::new(&backends));

        let forward = Forward {
            algorithm: Algorithm::Wrr,
            backends,
            scheduler,
        };

        Server {
            listen: vec!["127.0.0.1:0".parse().unwrap()],
            patterns: vec![Pattern {
                uri: String::from(uri),
                action: Action::Forward(forward),
            }],
        }
    }
}

pub mod files {
    //! Static files server configurations.

    use rxh::config::{Action, Pattern, Server};

    /// Serves files from `root` for all requests.
    pub fn serve(root: &str) -> Server {
        serve_at_uri(root, "/")
    }

    /// Serves files from `root` if the request URI matchees `uri`.
    pub fn serve_at_uri(root: &str, uri: &str) -> Server {
        Server {
            listen: vec!["127.0.0.1:0".parse().unwrap()],
            patterns: vec![Pattern {
                uri: String::from(uri),
                action: Action::Serve(String::from(root)),
            }],
        }
    }
}
