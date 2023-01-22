//! RXH is a reverse proxy, load balancer and static files server.

#![feature(ptr_from_ref)]
#![feature(is_some_and)]

mod http;
mod service;
mod sync;
mod task;

pub mod config;
pub mod sched;

use std::io;

pub use task::{
    master::Master,
    server::{Server, ShutdownState, State},
};

/// RXH version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Top level error to use for return types in the public API and main function.
#[derive(Debug)]
pub enum Error {
    /// Mostly related to reading or writing on sockets.
    Io(io::Error),

    /// An error while deserializing the config file.
    Toml(toml::de::Error),

    /// Error while processing HTTP requests.
    Http(hyper::Error),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "IO error: {err}"),
            Error::Toml(err) => write!(f, "TOML parse error: {err}"),
            Error::Http(err) => write!(f, "HTTP error: {err}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Error::Toml(value)
    }
}

impl From<hyper::Error> for Error {
    fn from(value: hyper::Error) -> Self {
        Error::Http(value)
    }
}
