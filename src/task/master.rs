use std::{
    future::{self, Future},
    net::SocketAddr,
    pin::Pin,
};

use tokio::sync::{broadcast, watch};

use crate::{config::NormalizedConfig, Server, State};

/// The master task is responsible for creating, spawning and shutting down all
/// the [`Server`] instances described in the configuration file. Both spawning
/// and shutting down are non-trivial due to all the options that the config
/// file provides.
///
/// # Replicas
///
/// The configuration file allows a single server to listen on multiple
/// IP addresses or ports:
///
/// ```toml
/// [[server]]
///
/// listen = ["127.0.0.1:8080", "127.0.0.1:8081"]
/// forward = "127.0.0.1:9000"
/// ```
///
/// Instead of passing this configuration "as is" to a [`Server`] and
/// managing multiple listeners within the [`Server`], we create a "replica"
/// for each listening address. A replica is just another [`Server`] with
/// the same configuration but a different listening socket. For example,
/// we could write the TOML above as follows:
///
/// ```toml
/// [[server]]
///
/// listen = "127.0.0.1:8080"
/// forward = "127.0.0.1:9000"
///
/// [[server]]
///
/// listen = "127.0.0.1:8081"
/// forward = "127.0.0.1:9000"
/// ```
///
/// By doing this, instead of having a [`Server`] with two listeners we have
/// two [`Server`] instances with a single listener. This removes one level
/// of notification forwarding, since in the first case we'd have to send
/// the shutdown notification to a [`Server`] and then the [`Server`] would
/// have to notify each one of its listeners, which in turn would have to
/// notify the tasks handling the requests.
///
/// However, if each server has a single listener, it does not need to spawn
/// additional tasks to run multiple listeners in parallel, which means the
/// server itself only has to forward the notification to the tasks handling
/// requests.
///
/// Here's a diagram using CTRL-C as the top shutdown event:
///
/// ```text
///                         +--------+
///                         | CTRL-C |
///                         +--------+
///                              |
///                              | Shutdown event (SIGINT)
///                              V
///                         +--------+
///                         | Master |
///                         +--------+
///                              |
///                              | Forward the signal to each server.
///                              |
///               +--------------+--------------+
///               |                             |
///               v                             v
///          +----------+                 +----------+
///          | Server 1 |                 | Server 2 |
///          +----------+                 +----------+
///               |                             |
///               | This is skipped             |
///               v                             v
///          +----------+                 +----------+
///          | Listener |                 | Listener |
///          +----------+                 +----------+
///               |                             |
///               | Notify request handlers     |
///               |                             |
///       +-------+-------+             +-------+-------+
///       |               |             |               |
///       v               v             v               v
/// +----------+   +----------+   +----------+   +----------+
/// | Task 1.1 |   | Task 1.2 |   | Taks 2.1 |   | Task 2.2 |
/// +----------+   +----------+   +----------+   +----------+
/// ```
///
/// The server doesn't need to notify the listener because it's not running on
/// a different [`tokio::task`], which makes things easier. So whenever we
/// encounter a server in the config that listens on multiple addresses we
/// simply clone that server multiple times giving each clone a different
/// listening address. See [`Server`] for more implementation details.
pub struct Master {
    /// All the servers that the master has spawned.
    servers: Vec<Server>,

    /// Subscriptions to state updates from each server.
    states: Vec<(SocketAddr, watch::Receiver<State>)>,

    /// Shutdown future. The master polls this future and when it's ready it
    /// sends the shutdown signal to all the servers, then waits for them to
    /// finish their pending tasks.
    shutdown: Pin<Box<dyn Future<Output = ()> + Send>>,

    /// Shutdown notifications channel. Spawned servers are subscribed to this
    /// channel and can receive the shutdown signal.
    shutdown_notify: broadcast::Sender<()>,
}

impl Master {
    /// Attempts to initialize all the servers specified in the configuration
    /// file or in the received `config`. The initialization only acquires and
    /// configures the TCP sockets, but does not listen or accept connections.
    /// See [`Server::init`] for more details.
    pub fn init(config: NormalizedConfig) -> Result<Self, crate::Error> {
        let mut servers = Vec::new();
        let mut states = Vec::new();
        let shutdown = Box::pin(future::pending());
        let (shutdown_notify, _) = broadcast::channel(1);

        for config in config.servers {
            let server = Server::init(config)?;
            states.push((server.socket_address(), server.subscribe()));
            servers.push(server);
        }

        Ok(Self {
            servers,
            states,
            shutdown,
            shutdown_notify,
        })
    }

    /// When `future` is ready, the graceful shutdown process begins. See
    /// [`Master`] definition, [`Server`] and [`crate::sync::notify`].
    pub fn shutdown_on(mut self, future: impl Future + Send + 'static) -> Self {
        self.servers = self
            .servers
            .into_iter()
            .map(|server| {
                let mut shutdown_notification = self.shutdown_notify.subscribe();
                server.shutdown_on(async move { shutdown_notification.recv().await })
            })
            .collect();

        self.shutdown = Box::pin(async move {
            future.await;
        });

        self
    }

    /// All the servers are put into `listen` mode and they start accepting
    /// connections.
    pub async fn run(self) -> Result<(), crate::Error> {
        let mut set = tokio::task::JoinSet::new();

        for server in self.servers {
            set.spawn(server.run());
        }

        let mut first_error = None;

        tokio::select! {
            Some(Ok(Err(err))) = set.join_next() => {
                first_error = Some(err);
                println!("Master => Received error while waiting for shutdown");
            }

            // TODO: Check for first join error. That means a server has panicked.

            _ = self.shutdown => {
                println!("Master => Sending shutdown signal to all servers");
            }
        }

        self.shutdown_notify.send(()).unwrap();

        while let Some(result) = set.join_next().await {
            if let Err(err) = result.unwrap() {
                first_error.get_or_insert(err);
            }
        }

        match first_error {
            None => Ok(()),
            Some(err) => Err(crate::Error::from(err)),
        }
    }

    /// Returns all the listening sockets.
    pub fn sockets(&self) -> Vec<SocketAddr> {
        self.states.iter().map(|(addr, _)| *addr).collect()
    }
}
