//! This module defines the architecture of RXH, which we call "master-server"
//! because each "worker" corresponds to a "server" in the config file. Since
//! we're working with [`tokio`], we refer to processing units as tasks, which
//! are defined at [`tokio::task`]. Tasks are light-weight non-blocking units
//! of execution and they are completely handled by Tokio, we don't need to
//! spawn threads or fork processes. We still need to synchronize access to
//! shared data and handle [`Send`] between threads, and we do that mostly
//! through message passing. See [`master`] and [`server`] for more details.

pub(crate) mod master;
pub(crate) mod server;
