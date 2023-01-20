//! Structs and enums derived from the config file using [`serde`].

mod deser;

use std::net::SocketAddr;

use deser::one_or_many;
use serde::{Deserialize, Serialize};

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
    #[serde(deserialize_with = "one_or_many")]
    pub listen: Vec<SocketAddr>,

    /// Patterns that this server should match against.
    #[serde(rename = "match")]
    pub patterns: Vec<Pattern>,
}

/// This is a single element of a `match` list in the configuration of a server.
/// See [`Server`] and [`deser`] module.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Pattern {
    /// URI prefix to match against.
    #[serde(default = "default::uri")]
    pub uri: String,

    /// Action to execute if this pattern matches the request.
    #[serde(flatten)]
    pub action: Action,
}

/// Describe what should be done when a request matches a pattern.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Forward the request to an upstream server or load balance between
    /// multiple of them.
    #[serde(deserialize_with = "one_or_many")]
    Forward(Vec<SocketAddr>),

    /// Serve static files from a root directory.
    Serve(String),
}

mod default {
    //! Default values for some configuration options.

    pub fn uri() -> String {
        String::from("/")
    }
}
