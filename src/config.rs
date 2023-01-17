use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// Global configuration, includes all servers.
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    /// Servers to spawn.
    #[serde(rename = "server")]
    pub servers: Vec<Server>,
}

/// Server specific configuration.
#[derive(Serialize, Deserialize, Debug)]
pub struct Server {
    /// TCP listener bind address.
    pub listen: SocketAddr,

    /// URI prefix. Used to forward requests to the target server only if the
    /// URI starts with this prefix, otherwise respond with HTTP 404.
    #[serde(default = "default::prefix")]
    pub prefix: String,

    /// Kind specific configuration.
    #[serde(flatten)]
    pub kind: Kind,
}

//// Server kind, for now only [`Proxy`] or [`Static`].
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum Kind {
    #[serde(rename = "proxy")]
    Proxy(Proxy),

    #[serde(rename = "static")]
    Static(Static),
}

/// Proxy server configuration.
#[derive(Serialize, Deserialize, Debug)]
pub struct Proxy {
    /// Proxy target, this is where incoming requests are forwarded.
    pub target: SocketAddr,
}

/// Static files server configuration.
#[derive(Serialize, Deserialize, Debug)]
pub struct Static {
    /// Root directory where files should served from.
    pub root: String,
}

mod default {
    ///! Default values for some configuration options.

    /// Default prefix means forward everything to target server.
    pub fn prefix() -> String {
        String::from("/")
    }
}
