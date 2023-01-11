#![feature(ptr_from_ref)]

mod config;
mod proxy;
mod request;
mod response;

pub mod server;

pub use config::Config;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
