#![feature(ptr_from_ref)]

mod config;
mod movable;
mod notify;
mod proxy;
mod request;
mod response;
mod server;

pub use config::Config;
pub use server::{Server, ShutdownState, State};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
