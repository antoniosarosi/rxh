#![feature(ptr_from_ref)]

mod config;
mod notification;
mod proxy;
mod request;
mod response;

pub mod server;

pub use config::Config;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
