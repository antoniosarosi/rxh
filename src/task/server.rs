use std::{future::Future, io, net::SocketAddr, pin::Pin, ptr};

use tokio::{
    net::{TcpListener, TcpSocket},
    sync::watch,
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
}

/// Represents the current state of the server.
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// Server has started but is not accepting connections yet.
    Starting,

    /// Server is accepting incoming connections.
    Listening,

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
    ///
    /// For the `replica` parameter see [`super::master::Master`], but basically
    /// it's a number that indicates which address should this server choose for
    /// listening, since the config file allows multiple addresses.
    pub fn init(config: config::Server, replica: usize) -> Result<Self, io::Error> {
        let (state, _) = watch::channel(State::Starting);

        let socket = if config.listen[replica].is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };

        #[cfg(not(windows))]
        socket.set_reuseaddr(true)?;

        socket.bind(config.listen[replica])?;

        // TODO: Hardcoded backlog, maybe this should be configurable.
        let listener = socket.listen(1024)?;
        let address = listener.local_addr().unwrap();

        let notifier = Notifier::new();

        // Don't shutdown on anything by default. CTRL-C will forcefully kill
        // the process.
        let shutdown = Box::pin(std::future::pending());

        Ok(Self {
            state,
            listener,
            config,
            address,
            notifier,
            shutdown,
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
    pub async fn run(self) -> Result<(), io::Error> {
        let Self {
            config,
            state,
            listener,
            notifier,
            shutdown,
            address,
            ..
        } = self;

        state.send_replace(State::Listening);

        // Leak the configuration to get a 'static lifetime, which we need to
        // spawn tokio tasks. Later when all tasks have finished, we'll drop this
        // value to avoid actual memory leaks.
        let config = Box::leak(Box::new(config));

        tokio::select! {
            result = Self::listen(listener, config, &notifier) => {
                if let Err(err) = result {
                    println!("Error while accepting connections: {err}");
                }
            }
            _ = shutdown => {
                println!("Server at {address} received shutdown signal");
            }
        }

        if let Ok(num_tasks) = notifier.send(Notification::Shutdown) {
            state.send_replace(State::ShuttingDown(ShutdownState::PendingConnections(
                num_tasks,
            )));
            notifier.collect_acknowledgements().await;
        }

        // SAFETY: Nobody is reading this configuration anymore because all tasks
        // have ended at this point, so there are no more references to this
        // address. It's an ugly hack, but we don't have to use Arc if we do this.
        unsafe {
            drop(Box::from_raw(ptr::from_ref(config).cast_mut()));
        }

        state.send_replace(State::ShuttingDown(ShutdownState::Done));

        Ok(())
    }

    /// Starts accepting incoming connections and processing HTTP requests.
    async fn listen(
        listener: TcpListener,
        config: &'static config::Server,
        notifier: &Notifier,
    ) -> Result<(), io::Error> {
        loop {
            let (stream, client_addr) = listener.accept().await?;
            let server_addr = stream.local_addr()?;
            let mut subscription = notifier.subscribe();
            println!("Connection from {client_addr}");

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
            });
        }
    }
}