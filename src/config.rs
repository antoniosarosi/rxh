use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use serde_json;
use tokio::{fs, sync::OnceCell};

/// Global configuration parsed from the JSON file. This is read only and
/// available during the entire duration of the program.
pub(crate) static CONFIG: OnceCell<Config> = OnceCell::const_new();

/// We need a pointer to the parsed configuration pretty much everywhere in the
/// code, but we don't want to access global variables since that would
/// complicate unit testing. We don't want static lifetime refs everywhere
/// either, so this is the solution. Evey struct that needs a pointer to the
/// configuration will hold a [`ConfigRef`] and it can access the actual
/// config whenever it wants, it doesn't care where it comes from, which makes
/// testing easier.
pub(crate) trait ConfigRef {
    fn get(&self) -> &Config;
}

/// Global configuration struct that implements [`ConfigRef`] by accessing
/// the global variable [`CONFIG`].
#[derive(Clone, Copy)]
pub(crate) struct GlobalConfig;

impl GlobalConfig {
    /// Parse JSON file and initialize configuration.
    /// TODO: Parameterize JSON file path.
    pub async fn try_init() -> Result<(), Box<dyn std::error::Error>> {
        CONFIG
            .get_or_try_init(|| async {
                let json = fs::read_to_string("rxh.json").await?;
                Ok(serde_json::from_str(&json)?)
            })
            .await
            .and_then(|_| Ok(()))
    }
}

impl ConfigRef for GlobalConfig {
    fn get(&self) -> &Config {
        // SAFETY: Global configuration is parsed and initialized at the
        // beginning of the program.
        unsafe { CONFIG.get().unwrap_unchecked() }
    }
}

/// Global configuration options parsed from the config file.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct Config {
    /// Proxy target, this is where incoming requests are forwarded.
    pub target: SocketAddr,

    /// TCP listener bind address.
    pub listen: SocketAddr,

    /// URI prefix. Used to forward requests to the target server only if the
    /// URI starts with this prefix, otherwise respond with HTTP 404.
    #[serde(default = "default::prefix")]
    pub prefix: String,
}

mod default {
    ///! Default values for some configuration options.

    /// Default prefix means forward everything to target server.
    pub fn prefix() -> String {
        String::from("/")
    }
}
