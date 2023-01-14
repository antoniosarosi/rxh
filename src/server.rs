use std::{future::Future, io, net::SocketAddr, ptr};

use tokio::net::{TcpListener, TcpSocket};

use crate::{
    config::Config,
    notification::{Message, Notifier},
    proxy::Proxy,
};

/// Creates and configures the socket that the server will use to listen but
/// does nothing else, in order to accept connections the returned future
/// must be polled.
///
/// ```no_run
/// #[tokio::main]
/// async fn main() -> Result<(), tokio::io::Error> {
///     let (address, server) = rxh::server::init(rxh::Config::default(), tokio::signal::ctrl_c())?;
///
///     // The returned future must be polled, otherwise it does nothing.
///     server.await?;
///
///     Ok(())
/// }
/// ```
#[must_use = "futures do nothing unless polled"]
pub fn init(
    config: Config,
    shutdown: impl Future,
) -> Result<(SocketAddr, impl Future<Output = Result<(), io::Error>>), io::Error> {
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
    let addr = listener.local_addr().unwrap();

    Ok((addr, master(listener, config, shutdown)))
}

/// The "Master" [`Future`] is responsible for accepting new connections and
/// spawning Tokio tasks to handle them properly, as well as gracefully stopping
/// the process. In order to perform graceful shutdowns, the master [`Future`]
/// notifies all the running tasks about the shutdown event and waits for their
/// acknowledgements. The tasks can only send the notification acknowledgement
/// when they are done processing requests from their assigned connection, which
/// causes the process to only exit when all remaining sockets are closed.
/// Here's a simple diagram describing this process:
///
/// ```text
///                     +--------+
///                     | Master |
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
async fn master(
    listener: TcpListener,
    config: Config,
    shutdown: impl Future,
) -> Result<(), io::Error> {
    // Leak the configuration to get a 'static lifetime, which we need to
    // spawn tokio tasks. Later when all tasks have finished, we'll drop this
    // value to avoid actual memory leaks.
    let config: &'static Config = Box::leak(Box::new(config));

    let notifier = Notifier::new();

    let server = Server::new(listener, &config, &notifier);

    tokio::select! {
        _result = server.listen() => {
            // TODO: Handle accept errors.
        }
        _ = shutdown => {
            println!("Shutting down");
        }
    }

    if let Ok(num_tasks) = notifier.send(Message::Shutdown) {
        println!("{num_tasks} pending connections, waiting for them to end...");
        notifier.collect_acknowledgements().await;
    }

    // SAFETY: Nobody is reading this configuration anymore because all tasks
    // have ended at this point, so there are no more references to this
    // address. It's an ugly hack, but we don't have to use Arc if we do this.
    unsafe {
        drop(Box::from_raw(ptr::from_ref(config).cast_mut()));
    }

    Ok(())
}

/// TCP server that listens for incoming connections and spawns a Tokio task
/// for each one of them.
struct Server<'a> {
    listener: TcpListener,
    config: &'static Config,
    notifier: &'a Notifier,
}

impl<'a> Server<'a> {
    /// Creates a new [`Server`] using `listener` to accept connections.
    pub fn new(listener: TcpListener, config: &'static Config, notifier: &'a Notifier) -> Self {
        Self {
            listener,
            config,
            notifier,
        }
    }

    /// Starts accepting incoming connections and processing HTTP requests.
    async fn listen(&self) -> Result<(), io::Error> {
        loop {
            let (stream, client_addr) = self.listener.accept().await?;
            let server_addr = stream.local_addr()?;
            let config = self.config;
            let notification = self.notifier.subscribe();

            tokio::task::spawn(async move {
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(
                        stream,
                        Proxy::new(config, client_addr, server_addr, notification),
                    )
                    .with_upgrades()
                    .await
                {
                    println!("Failed to serve connection: {:?}", err);
                }
            });
        }
    }
}
