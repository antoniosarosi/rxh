mod config;
mod proxy;
mod request;
mod response;
mod server;

use tokio::fs;

use crate::server::Server;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = serde_json::from_str(&fs::read_to_string("rxh.json").await?)?;
    Server::new(Box::leak(config)).listen().await
}
