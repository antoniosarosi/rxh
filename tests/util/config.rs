//! Configuration factories for integrations tests.

pub mod proxy {
    use std::net::SocketAddr;

    use rxh::{
        config::{Action, Algorithm, Backend, Forward, Pattern, Scheduler, Server},
        sched::WeightedRoundRobin,
    };

    pub fn target(address: SocketAddr) -> Server {
        target_with_uri(address, "/")
    }

    pub fn target_with_uri(address: SocketAddr, uri: &str) -> Server {
        let backends = vec![Backend { address, weight: 1 }];
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
