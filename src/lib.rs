#![feature(ptr_from_ref)]
#![feature(is_some_and)]

mod http;
mod notify;
mod server;
mod service;

pub mod config;

pub use server::{Server, ShutdownState, State};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
