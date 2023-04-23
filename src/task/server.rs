use std::{future::Future, io, net::SocketAddr, pin::Pin, ptr, sync::Arc};

use tokio::{
    net::{TcpListener, TcpSocket},
    sync::{watch, Semaphore},
};

use crate::{
    config,
    service::Rxh,
    sync::notify::{Notification, Notifier},
};

/// The [`Server`] struct represents a particular `[[server]]` instance from the
/// config file. It is responsible for accepting new connections and spawning
/// Tokio tasks to handle them properly, as well as gracefully stopping the
/// running tasks. In order to perform graceful shutdowns, the [`Server`]
/// notifies all the running tasks about the shutdown event and waits for their
/// acknowledgements. See [`Notifier`] for further details. Here's a simple
/// diagram describing the process:
///
/// ```text
///                     +--------+
///                     | Server |
///                     +--------+
///                         |
///                         v
///                     +--------+
///                +--- | Select | ---+
///                |    +--------+    |
///                v                  v
///          +----------+       +----------+
///          |  Accept  |       | Shutdown |
///          +----------+       +----------+
///                |                  |
///                v                  v
///          +----------+       +----------+
///          |  Spawn   |       |  Notify  |
///          +----------+       +----------+
///                |                  |
///                v                  v
/// +--------+   +--------+   +--------+   +--------+
/// | Task 1 |   | Task 2 |   | Task 3 |   | Task 4 |
/// +--------+   +--------+   +--------+   +--------+
/// ```
pub struct Server {
    /// State updates channel. Subscribers can use this to check the current
    /// [`State`] of this server.
    state: watch::Sender<State>,

    /// TCP listener used to accept connections.
    listener: TcpListener,

    /// Configuration for this server.
    config: config::Server,

    /// Socket address used by this server to listen for incoming connections.
    address: SocketAddr,

    /// [`Notifier`] object used to send notifications to tasks spawned by
    /// this server.
    notifier: Notifier,

    /// Shutdown future, this can be anything, which allows us to easily write
    /// integration tests. When this future completes, the server starts the
    /// shutdown process.
    shutdown: Pin<Box<dyn Future<Output = ()> + Send>>,

    /// Connections are limited to a maximum number. In order to allow a new
    /// connection we'll have a acquire a permit from the semaphore.
    connections: Arc<Semaphore>,
}

/// Represents the current state of the server.
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// Server has started but is not accepting connections yet.
    Starting,

    /// Server is accepting incoming connections.
    Listening,

    /// Maximum number of connections reached.
    MaxConnectionsReached(usize),

    /// Server is gracefully shutting down.
    ShuttingDown(ShutdownState),
}

/// Represents a state in the graceful shutdown process.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ShutdownState {
    /// The server has received the shutdown signal and won't accept more
    /// connections, but it will still process data for currently connected
    /// sockets.
    PendingConnections(usize),

    /// Shutdown process complete.
    Done,
}

impl Server {
    /// Initializes a [`Server`] with the given `config`. This process makes
    /// sure that the listening address can be used and configures a socket
    /// for that address, but does not accept connections yet. In order to
    /// process incoming connections, [`Server::run`] must be called and
    /// `await`ed. We do it this way because we use the port 0 for integration
    /// tests, which allows the OS to pick any available port, but we still want
    /// to know which port the server is using.
    pub fn init(config: config::Server) -> Result<Self, io::Error> {
        let (state, _) = watch::channel(State::Starting);

        let socket = if config.listen.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };

        #[cfg(not(windows))]
        socket.set_reuseaddr(true)?;

        socket.bind(config.listen)?;

        // TODO: Hardcoded backlog, maybe this should be configurable.
        let listener = socket.listen(1024)?;

        // If the TCP port is 0 then the OS will choose a valid one.
        let address = listener.local_addr().unwrap();

        let notifier = Notifier::new();

        // Don't shutdown on anything by default. CTRL-C will forcefully kill
        // the process.
        let shutdown = Box::pin(std::future::pending());

        let connections = Arc::new(Semaphore::new(config.max_connections));

        Ok(Self {
            state,
            listener,
            config,
            address,
            notifier,
            shutdown,
            connections,
        })
    }

    /// The [`Server`] will poll the given `future` and whenever it completes,
    /// the graceful shutdown process starts. If only one server is
    /// instantiated, this could be called with [`tokio::signal::ctrl_c`], but
    /// it can be any [`Future`] since we need customization for integration
    /// tests and spawning multiple servers using [`super::master::Master`].
    pub fn shutdown_on(mut self, future: impl Future + Send + 'static) -> Self {
        self.shutdown = Box::pin(async move {
            future.await;
        });

        self
    }

    /// Address of the listening socket. This is necessary for obtaining the
    /// actual address in cases port 0 was used.
    pub fn socket_address(&self) -> SocketAddr {
        self.address
    }

    /// By subscribing to this server the caller obtains a channel where the
    /// current state of the server can be read. This allows the server and
    /// caller to run on separate Tokio tasks while still allowing the caller
    /// to read the state.
    pub fn subscribe(&self) -> watch::Receiver<State> {
        self.state.subscribe()
    }

    /// This is the entry point, by calling and `await`ing this function the
    /// server starts to process connections.
    pub async fn run(self) -> Result<(), crate::Error> {
        let Self {
            mut config,
            state,
            listener,
            notifier,
            shutdown,
            address,
            connections,
        } = self;

        let log_name = if let Some(ref id) = config.name {
            format!("{address} ({id})")
        } else {
            address.to_string()
        };

        config.log_name = log_name.clone();

        state.send_replace(State::Listening);
        println!("{log_name} => Listening for requests");

        // Leak the configuration to get a 'static lifetime, which we need to
        // spawn tokio tasks. Later when all tasks have finished, we'll drop this
        // value to avoid actual memory leaks.
        let config = Box::leak(Box::new(config));

        let listener = Listener {
            config,
            connections,
            listener,
            notifier: &notifier,
            state: &state,
        };

        tokio::select! {
            result = listener.listen() => {
                if let Err(err) = result {
                    println!("{log_name} => Error while accepting connections: {err}");
                }
            }
            _ = shutdown => {
                println!("{log_name} => Received shutdown signal");
            }
        }

        // Drop the listener to stop accepting new connections. This will cause
        // a "Connection Refused" error on any new client socket that attempts
        // to connect. Already connected sockets will still be able to send and
        // receive data.
        drop(listener);

        if let Ok(num_tasks) = notifier.send(Notification::Shutdown) {
            println!("{log_name} => Can't shutdown yet, {num_tasks} pending connections");
            state.send_replace(State::ShuttingDown(ShutdownState::PendingConnections(
                num_tasks,
            )));
            notifier.collect_acknowledgements().await;
        }

        // SAFETY: Nobody is reading this configuration anymore because all
        // tasks have ended at this point, so there are no more references to
        // this address. It's an ugly hack, but we don't have to use Arc if we
        // do this, we can simply skip the reference counting and avoid atomic
        // operations.
        unsafe {
            drop(Box::from_raw(ptr::from_ref(config).cast_mut()));
        }

        state.send_replace(State::ShuttingDown(ShutdownState::Done));
        println!("{log_name} => Shutdown complete");

        Ok(())
    }
}

/// Listens for incoming connections and spawns tasks to handle them if permits
/// are available.
struct Listener<'a> {
    /// Underlying TCP listener. We take ownership of this so that when this
    /// struct is dropped the socket is also dropped and we stop accepting
    /// connections.
    listener: TcpListener,

    /// Reference to the configuration of this server.
    config: &'static config::Server,

    /// Needed to obtain subscriptions and pass them down to request handler
    /// tasks.
    notifier: &'a Notifier,

    /// Used to update the state when max connections are reached.
    state: &'a watch::Sender<State>,

    /// Connections permits.
    connections: Arc<Semaphore>,
}

impl<'a> Listener<'a> {
    pub async fn listen(&self) -> Result<(), crate::Error> {
        loop {
            // Move out of config to get a static lifetime that we can pass down
            // to the new Tokio task.
            let config = self.config;

            let mut notify_listening_again = false;

            if self.connections.available_permits() == 0 {
                println!(
                    "{} => Reached max connections: {}",
                    config.log_name, config.max_connections
                );
                self.state
                    .send_replace(State::MaxConnectionsReached(config.max_connections));
                notify_listening_again = true;
            }

            // We don't close the semaphore so unwrapping is OK.
            let permit = self.connections.clone().acquire_owned().await.unwrap();

            // Once we've obtainied a permite we can start listening again if
            // we stopped before.
            if notify_listening_again {
                println!("{} => Accepting connections again", config.log_name);
                self.state.send_replace(State::Listening);
            }

            let (stream, client_addr) = self.listener.accept().await?;
            let mut subscription = self.notifier.subscribe();
            let server_addr = stream.local_addr()?;

            tokio::task::spawn(async move {
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(stream, Rxh::new(config, client_addr, server_addr))
                    .with_upgrades()
                    .await
                {
                    println!("Failed to serve connection: {:?}", err);
                }

                if let Some(Notification::Shutdown) = subscription.receive_notification() {
                    subscription.acknowledge_notification().await;
                }

                // Permit is dropped only when the accepted socket is done
                // sending and receiving data.
                drop(permit);
            });
        }
    }
}
