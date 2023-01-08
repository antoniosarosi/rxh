mod config;
mod proxy;
mod request;
mod response;
mod server;

use crate::{config::GlobalConfig, server::Server};

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    GlobalConfig::try_init().await?;
    Server::new(GlobalConfig).listen().await
}
