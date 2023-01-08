mod config;
mod proxy;
mod request;
mod response;
mod server;

pub use config::Config;
pub use server::Server;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
