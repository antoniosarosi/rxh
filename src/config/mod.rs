//! Structs and enums derived from the config file using [`serde`].

mod deser;

use std::{fmt::Debug, net::SocketAddr};

use deser::{BackendOption, ForwardOption};
use serde::{Deserialize, Serialize};

use crate::sched::WeightedRoundRobin;

/// This struct represents the entire configuration file, which describes a list
/// of servers and their particular configuration options. For example, this
/// configuration:
///
/// ```toml
/// [[server]]
///
/// listen = "127.0.0.1:8000"
/// forward = "127.0.0.1:8080"
///
/// [[server]]
///
/// listen = "127.0.0.1:9000"
/// serve = "/home/user/website"
/// ```
///
/// Should result in a [`Vec`] containing two [`Server`] elements after
/// deserializing.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// List of all servers.
    #[serde(rename = "server")]
    pub servers: Vec<Server>,
}

/// Description of a single server instance in the config file. The server
/// allows a "simple" pattern or multiple patterns. For example:
///
/// ```toml
/// # Simple pattern.
///
/// [[server]]
///
/// listen = "127.0.0.1:8000"
/// forward = "127.0.0.1:9000"
/// uri = "/api"
///
/// # Multiple patterns using "match".
///
/// [[server]]
///
/// listen = "128.0.01:8001"
///
/// match = [
///     { uri = "/front", serve = "/home/website" },
///     { uri = "/brack", forward = "127.0.0.1:9001" },
/// ]
/// ```
///
/// This is not provided by [`serde`], see [`deser`] module for implementation
/// details.
#[derive(Serialize, Debug, Clone)]
pub struct Server {
    /// Socket addresses where this server listens.
    pub listen: Vec<SocketAddr>,

    /// Patterns that this server should match against.
    #[serde(rename = "match")]
    pub patterns: Vec<Pattern>,

    #[serde(default = "default::connections")]
    pub connections: usize,

    /// Optional server name to show in logs and forwarded requests.
    pub name: Option<String>,

    /// Log name inlcudes the IP address of the listening socket and also the
    /// optional name set by the user.
    #[serde(skip)]
    pub log_name: String,
}

/// This is a single element of a `match` list in the configuration of a server.
/// See [`Server`] and [`deser`] module.
///
/// ```toml
/// [[server]]
///
/// listen = "127.0.0.1:8000"
///
/// match = [
///     { uri = "/front", serve = "/home/website" },    # This is a Pattern
///     { uri = "/brack", forward = "127.0.0.1:9001" }, # This is another Pattern
/// ]
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Pattern {
    /// URI prefix to match against.
    #[serde(default = "default::uri")]
    pub uri: String,

    /// Action to execute if this pattern matches the request.
    #[serde(flatten)]
    pub action: Action,
}

/// One element in the "forward" list. This represents an upstream server and
/// when multiple of them are present load balancing has to be performed.
///
/// ```toml
/// /// [[server]]
///
/// listen = "127.0.0.1:8000"
/// forward = [
///     { address = "127.0.0.1:8080", weight = 1 }, # This is a Backend
///     { address = "127.0.0.1:8081", weight = 2 }, # This is another Backend
/// ]
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(from = "BackendOption")]
pub struct Backend {
    /// Address of the upstream server.
    pub address: SocketAddr,

    /// Some algorithms such as WRR (Weighted Round Robin) require each server
    /// to define a weight. For example, a server with 4 cores can have a weight
    /// of 1 while a server with 8 cores can have a weight of 2.
    pub weight: usize,
}

/// Algorithm that should be used for load balancing. For now we only implement
/// WRR, so there's no point in specifying this, but the syntax is as follows:
///
/// ```toml
/// [[server]]
///
/// listen = "127.0.0.1:8000"
///
/// [[server.forward]]
///
/// algorithm = "WRR"
/// backends = [
///     { address = "127.0.0.1:8080", weight = 1 },
///     { address = "127.0.0.1:8081", weight = 2 },
/// ]
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Algorithm {
    #[serde(rename = "WRR")]
    Wrr,
}

/// This is used mainly to avoid working with `dyn` traits. We don't know the
/// exact scheduler type at compile time because it's specified in the config
/// file and each server could have a different one (for now, there is only
/// one, but hey, design for scaling). Instead of a `dyn` trait which causes
/// issues because it has to be boxed as we don't know the size at compile time
/// and it's not [`Send`] and [`Sync`], we'll store the scheduler inside an
/// enum. We should look for another solution if we end up having many different
/// schedulers, but this works for now.
#[derive(Debug)]
pub enum Scheduler {
    Wrr(WeightedRoundRobin),
}

impl Scheduler {
    /// Creates a new scheduler from the given [`Algorithm`].
    pub fn from(algorithm: Algorithm, backends: &Vec<Backend>) -> Self {
        match algorithm {
            Algorithm::Wrr => Scheduler::Wrr(WeightedRoundRobin::new(backends)),
        }
    }

    /// Convinience for accessing the underlying scheduler.
    pub fn next_server(&self) -> SocketAddr {
        let scheduler = match self {
            Self::Wrr(wrr) => wrr,
        };

        use crate::sched::Scheduler;
        scheduler.next_server()
    }
}

/// Proxy specific configuration. This container is used to deserialize the
/// config:
///
/// ```toml
/// [[server]]
///
/// # This is the Forward struct
/// [[server.forward]]
///
/// algorithm = "WRR"
/// backends = [
///     { address = "127.0.0.1:8080", weight = 1 },
///     { address = "127.0.0.1:8081", weight = 2 },
/// ]
/// ```
///
/// But it's probably not necessary as we could store all this information
/// inside a [`Scheduler`]. We'll leave it here to match the config file and
/// keep it symmetric.
#[derive(Serialize, Deserialize, Debug)]
#[serde(from = "ForwardOption")]
pub struct Forward {
    /// Upstream servers.
    pub backends: Vec<Backend>,

    /// Algorithm used for load balancing.
    pub algorithm: Algorithm,

    /// Scheduler wrapper to avoid using boxed `dyn` traits.
    #[serde(skip)]
    pub scheduler: Scheduler,
}

impl Clone for Forward {
    fn clone(&self) -> Self {
        Self {
            backends: self.backends.clone(),
            algorithm: self.algorithm.clone(),
            scheduler: Scheduler::from(self.algorithm, &self.backends),
        }
    }
}

/// Describes what should be done when a request matches a pattern. This
/// option is flattened to remove one level of identation in the config files.
/// Here's an example of simple actions:
///
/// ```toml
/// [[server]]
///
/// listen = "127.0.0.1:8000"
///
/// match = [
///     { uri = "/front", serve = "/home/website" },    # Serve action
///     { uri = "/brack", forward = "127.0.0.1:9001" }, # Forward action
/// ]
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Forward the request to an upstream server or load balance between
    /// multiple of them.
    Forward(Forward),

    /// Serve static files from a root directory.
    Serve(String),
}

mod default {
    //! Default values for some configuration options.

    pub fn uri() -> String {
        String::from("/")
    }

    pub fn connections() -> usize {
        1024
    }
}
