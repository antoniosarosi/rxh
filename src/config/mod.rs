mod deser;

use std::net::SocketAddr;

use deser::one_or_many;
use serde::{Deserialize, Serialize};

/// This struct represents the entire configuration file, which describes a list
/// of servers and their particular configuration options.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// List of all servers.
    #[serde(rename = "server")]
    pub servers: Vec<Server>,
}

/// Description of a single server in the config file.
#[derive(Serialize, Debug, Clone)]
pub struct Server {
    /// Socket addresses where this server listens.
    #[serde(deserialize_with = "one_or_many")]
    pub listen: Vec<SocketAddr>,

    /// Patterns that this server should match against.
    #[serde(rename = "match")]
    pub patterns: Vec<Pattern>,
}

/// This is a single instance of a `match` in the configuration of a server.
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
