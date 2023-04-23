//! Structs and enums derived from the config file using [`serde`].

mod deser;

use std::{fmt::Debug, net::SocketAddr};

use deser::{BackendOption, ForwardOption};
use serde::{Deserialize, Serialize};

use crate::sched::{self, Scheduler};

/// This struct represents the entire configuration file, which describes a list
/// of servers and their particular configuration options. For example, this
/// configuration:
///
/// ```toml
/// [[server]]
///
/// listen = ["127.0.0.1:7000", "127.0.0.1:8000"]
/// forward = "127.0.0.1:8080"
///
/// [[server]]
///
/// listen = "127.0.0.1:9000"
/// serve = "/home/user/website"
/// ```
///
/// Should result in a [`Vec`] containing two [`Server`] elements after
/// deserializing. Further processing is necessary to account for server
/// instances with multiple listening addresses. See [`Server`] and
/// [`crate::task::master`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// List of all servers.
    #[serde(rename = "server")]
    pub servers: Vec<Server>,
}

impl Config {
    pub fn replicate_servers(self) -> Self {
        let servers = self
            .servers
            .iter()
            .flat_map(|server| server.replicate())
            .collect();

        Self { servers }
    }
}

/// Description of a single server instance in the config file. The server can
/// have multiple listening addresses and allows a "simple" pattern or multiple
/// patterns. For example:
///
/// ```toml
/// # Simple pattern, multiple addresses.
///
/// [[server]]
///
/// listen = ["127.0.0.1:7000", "127.0.0.1:8000"]
/// forward = "127.0.0.1:9000"
/// uri = "/api"
///
/// # Multiple patterns using "match", single address.
///
/// [[server]]
///
/// listen = "128.0.01:6000"
///
/// match = [
///     { uri = "/front", serve = "/home/website" },
///     { uri = "/brack", forward = "127.0.0.1:9001" },
/// ]
/// ```
///
/// Deserialization of this syntax is not provided by [`serde`], see [`deser`]
/// module for implementation details.
///
/// # Multiple Listening Addresses
///
/// The configuration file allows a server to listen on multiple addresses.
/// However, in terms of programming, it is easier to reason about multiple
/// servers each listening on one address only. The idea is turning this:
///
/// ```toml
/// [[server]]
/// listen = ["127.0.0.1:7000", "127.0.0.1:8000"]
/// forward = "127.0.0.1:8080"
/// ```
///
/// into this something like this:
///
/// ```toml
/// [[server]]
/// listen = "127.0.0.1:7000"
/// forward = "127.0.0.1:8080"
///
/// [[server]]
/// listen = "127.0.0.1:8000"
/// forward = "127.0.0.1:8080"
/// ```
///
/// By doing so, graceful shutdown becomes easier since we only have to send the
/// shutdown signal once to each server. On the other hand, if we allowed each
/// server to manage multiple listeners itself, then the server would have to
/// forward the shutdown signal to all its listeners, adding another lever of
/// indirection. See [`crate::task::master::Master`] for more details about
/// signals.
///
/// # Replicas
///
/// In order to match the config file (multiple addresses per server) with the
/// concept of having multiple servers listening on one address only, we just
/// "replicate" the server as many times as needed. Each replica stores an array
/// of all the listening addresses and also a *replica number* or ID, which is
/// basically a valid index in the addresses array. Note that this is only
/// needed at runtime, the user doesn't need to think about replicas. The user
/// would write this in the config file:
///
/// ```toml
/// [[server]]
/// listen = ["127.0.0.1:7000", "127.0.0.1:8000"]
/// forward = "127.0.0.1:8080"
/// ```
///
/// And we turn it into this at runtime:
///
/// ```toml
/// [[server]]
/// listen = ["127.0.0.1:7000", "127.0.0.1:8000"]
/// forward = "127.0.0.1:8080"
/// replica = 0 # listen[0] is the address of this replica.
///
/// [[server]]
/// listen = ["127.0.0.1:7000", "127.0.0.1:8000"]
/// forward = "127.0.0.1:8080"
/// replica = 1 # listen[1] is the address of this replica.
/// ```
///
/// This also allows each replica to know its peers addresses, or the amount of
/// replicas available.
#[derive(Serialize, Debug, Clone)]
pub struct Server {
    /// Socket addresses where this server listens.
    pub listen: Vec<SocketAddr>,

    /// Replica number. Must be in the range `0..self.listen.len()`.
    pub replica: usize,

    /// Patterns that this server should match against.
    #[serde(rename = "match")]
    pub patterns: Vec<Pattern>,

    /// Connections limit.
    #[serde(default = "default::max_connections")]
    pub max_connections: usize,

    /// Optional server name to show in logs and forwarded requests.
    pub name: Option<String>,

    /// Log name inlcudes the IP address of the listening socket and also the
    /// optional name set by the user.
    #[serde(skip)]
    pub log_name: String,
}

impl Server {
    pub fn replicate(&self) -> Vec<Self> {
        let mut replicas = Vec::new();

        for replica in 0..self.listen.len() {
            replicas.push(Self {
                replica,
                ..self.clone()
            })
        }

        replicas
    }
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
#[derive(Serialize, Deserialize)]
#[serde(from = "ForwardOption")]
pub struct Forward {
    /// Upstream servers.
    pub backends: Vec<Backend>,

    /// Algorithm used for load balancing.
    pub algorithm: Algorithm,

    /// Load balancing scheduler.
    #[serde(skip)]
    pub scheduler: Box<dyn Scheduler + Sync + Send>,
}

impl Debug for Forward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Forward")
            .field("backends", &self.backends)
            .field("algorithm", &self.algorithm)
            .finish()
    }
}

impl Clone for Forward {
    fn clone(&self) -> Self {
        Self {
            backends: self.backends.clone(),
            algorithm: self.algorithm.clone(),
            scheduler: sched::make(self.algorithm, &self.backends),
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

    pub fn max_connections() -> usize {
        1024
    }
}
