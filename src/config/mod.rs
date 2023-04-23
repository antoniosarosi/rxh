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
/// Should result in a [`Vec`] containing two [`MultiServer`] elements after
/// deserializing.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// List of all servers.
    #[serde(rename = "server")]
    pub servers: Vec<MultiServer>,
}

impl Config {
    /// Consumes `self` returning an instance of [`NormalizedConfig`].
    pub fn into_normalized(self) -> NormalizedConfig {
        let servers = self
            .servers
            .iter()
            .flat_map(|server| server.split())
            .collect();

        NormalizedConfig { servers }
    }
}

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
/// into this:
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
/// By doing so graceful shutdown becomes easier since we only have to send the
/// shutdown signal to each server instance. If we allowed each server to manage
/// multiple listeners itself, then the server would have to forward the
/// shutdown signal to its listeners, adding another lever of indirection. See
/// [`crate::task::master::Master`] for more details.
pub struct NormalizedConfig {
    /// List of normalized server instances (one address only).
    pub servers: Vec<Server>,
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
/// This is not provided by [`serde`], see [`deser`] module for implementation
/// details.
#[derive(Serialize, Debug, Clone)]
pub struct MultiServer {
    /// Socket addresses where this server listens.
    pub listen: Vec<SocketAddr>,

    /// Rest of server configuration. The `listen` field in [`Server`] should
    /// have a temporary value like `127.0.0.1:0` while it is stored here.
    #[serde(flatten)]
    pub shared: Server,
}

impl MultiServer {
    /// Creates an instance of [`Self`] with the given addresses and options.
    pub fn new(listen: Vec<SocketAddr>, shared: Server) -> Self {
        Self { listen, shared }
    }

    /// Turns this [`MultiServer`] into multiple normalized servers (one
    /// listening address only).
    pub fn split(&self) -> Vec<Server> {
        self.listen
            .iter()
            .map(|address| Server {
                listen: *address,
                ..self.shared.clone()
            })
            .collect()
    }
}

/// Normalized server configuration used at runtime. Everything from the config
/// file is present here except that there's only one listening address. See
/// [`NormalizedConfig`].
#[derive(Serialize, Debug, Clone)]
pub struct Server {
    /// TCP address to listen on.
    #[serde(skip)]
    pub listen: SocketAddr,

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
