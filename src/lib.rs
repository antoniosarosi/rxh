#![feature(ptr_from_ref)]
#![feature(is_some_and)]

mod http;
mod service;
mod sync;
mod task;

pub mod config;
pub mod sched;

pub use task::{
    master::Master,
    server::{Server, ShutdownState, State},
};

/// RXH version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
