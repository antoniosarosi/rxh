//! Load balancing and scheduler implementations.

use std::net::SocketAddr;

mod wrr;

pub use wrr::WeightedRoundRobin;

use crate::config::{Algorithm, Backend};

/// A scheduler provides an algorithm for load balancing between multiple
/// backend servers.
pub trait Scheduler {
    /// Returns the address of the server that should process the next request.
    fn next_server(&self) -> SocketAddr;

    // Notify the scheduler when a server has processed a request. This is
    // useful for implementing load balancing algorithms such as "Least
    // Connections".
    // fn request_processed(server: SocketAddr);
}

/// [`Scheduler`] factory.
pub fn make(algorithm: Algorithm, backends: &Vec<Backend>) -> Box<dyn Scheduler + Send + Sync> {
    Box::new(match algorithm {
        Algorithm::Wrr => WeightedRoundRobin::new(backends),
    })
}
