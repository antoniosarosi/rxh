pub use rxh::{Config, Server};
use serde_json;
use tokio::{fs, sync::OnceCell};

/// Global configuration parsed from the JSON file. This is read only and
/// available during the entire duration of the program.
static CONFIG: OnceCell<Config> = OnceCell::const_new();

/// Reads and parses the config file.
async fn try_init_config() -> Result<&'static Config, Box<dyn std::error::Error>> {
    CONFIG
        .get_or_try_init(|| async {
            let json = fs::read_to_string("rxh.json").await?;
            Ok(serde_json::from_str(&json)?)
        })
        .await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = try_init_config().await?;
    Server::new(config).listen().await
}
