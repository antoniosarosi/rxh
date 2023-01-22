use std::net::SocketAddr;

use super::Scheduler;
use crate::{config::Backend, sync::ring::Ring};

/// Classical Weighted Round Robin (WRR) algorithm. Each backend server is
/// assigned a weight to distinguish its processing capabilities from others.
/// The normal Round Robin (RR) algorithm doesn't care about the processing
/// power of each server, so if we have 3 backend servers A, B and C and receive
/// 6 requests, this is how RR schedules them: `[A, B, C, A, B, C]`.
///
/// On the other hand, WRR sends more requests to the servers that have more
/// computing power. If we have 3 servers A, B and C with weights 1, 3 and 2,
/// this is how WRR would schedule the 6 requests from before:
/// `[A, B, B, B, C, C]`.
#[derive(Debug)]
pub struct WeightedRoundRobin {
    /// Pre-computed complete cycle of requests. We know exactly where each
    /// request is going to be sent just by looking at the weight of each server
    /// once, so we can calculate one cycle at the beginning of the program and
    /// then return values from it.
    cycle: Ring<SocketAddr>,
}

impl WeightedRoundRobin {
    /// Creates and initializes a new [`WeightedRoundRobin`] scheduler.
    pub fn new(backends: &Vec<Backend>) -> Self {
        let mut cycle = Vec::new();

        // TODO: Interleaved WRR
        for backend in backends {
            let mut weight = backend.weight;
            while weight > 0 {
                cycle.push(backend.address);
                weight -= 1;
            }
        }

        Self {
            cycle: Ring::new(cycle),
        }
    }
}

impl Scheduler for WeightedRoundRobin {
    fn next_server(&self) -> SocketAddr {
        self.cycle.next_as_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_round_robin() {
        let backends = vec![
            ("127.0.0.1:8080", 1),
            ("127.0.0.1:8081", 3),
            ("127.0.0.1:8082", 2),
        ];

        let expected = vec![
            "127.0.0.1:8080",
            "127.0.0.1:8081",
            "127.0.0.1:8081",
            "127.0.0.1:8081",
            "127.0.0.1:8082",
            "127.0.0.1:8082",
        ];

        let wrr = WeightedRoundRobin::new(
            &backends
                .iter()
                .map(|(addr, weight)| Backend {
                    address: addr.parse().unwrap(),
                    weight: *weight,
                })
                .collect(),
        );

        for server in expected {
            assert_eq!(server, wrr.next_server().to_string());
        }
    }
}
