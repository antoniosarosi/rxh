use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// Global configuration options parsed from the config file.
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    /// Proxy target, this is where incoming requests are forwarded.
    pub target: SocketAddr,

    /// TCP listener bind address.
    pub listen: SocketAddr,

    /// URI prefix. Used to forward requests to the target server only if the
    /// URI starts with this prefix, otherwise respond with HTTP 404.
    #[serde(default = "default::prefix")]
    pub prefix: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target: "0.0.0.0:8080".parse().unwrap(),
            listen: "0.0.0.0:8100".parse().unwrap(),
            prefix: default::prefix(),
        }
    }
}

mod default {
    ///! Default values for some configuration options.

    /// Default prefix means forward everything to target server.
    pub fn prefix() -> String {
        String::from("/")
    }
}
