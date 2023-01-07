use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// Global configuration options parsed from the config file.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Config {
    /// Proxy target, this is where incoming requests are forwarded.
    pub target: SocketAddr,

    /// TCP listener bind address.
    pub listen: SocketAddr,

    /// URI prefix. Used to forward requests to the target server only if the
    /// URI starts with this prefix, otherwise respond with HTTP 404.
    #[serde(default = "default_prefix")]
    pub prefix: String,
}

/// Default prefix means forward everything to target server.
fn default_prefix() -> String {
    String::from("/")
}
