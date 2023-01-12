use std::{future::Future, io, net::SocketAddr, ptr};

use tokio::net::{TcpListener, TcpSocket};

use crate::{config::Config, proxy::Proxy};

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

/// "Master" [`Future`], responsible for spawning tasks to handle connections
/// and gracefully shutting down all tasks.
async fn master(
    listener: TcpListener,
    config: Config,
    shutdown: impl Future,
) -> Result<(), io::Error> {
    // Leak the configuration to get a 'static lifetime, which we need to
    // spawn tokio tasks. Later when all tasks have finished, we'll drop this
    // value to avoid actual memory leaks.
    let config = Box::leak(Box::new(config));

    // let mut listen_result = Ok(());

    tokio::select! {
        _result = listen(listener, config) => {
            println!("End");
            // listen_result = result;
        }
        _ = shutdown => {
            println!("Shutting down");
        }
    }

    // TODO: Wait for all tasks to finish.

    // SAFETY: Nobody is reading this configuration anymore because all tasks
    // have ended at this point, so there are no more references to this
    // address. It's an ugly hack, but we don't have to use Arc if we do this.
    unsafe {
        drop(Box::from_raw(ptr::from_ref(config).cast_mut()));
    }

    Ok(())
}

/// Starts accepting incoming connections and processing HTTP requests.
async fn listen(listener: TcpListener, config: &'static Config) -> Result<(), io::Error> {
    loop {
        let (stream, client_addr) = listener.accept().await?;
        let server_addr = stream.local_addr()?;

        tokio::task::spawn(async move {
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(stream, Proxy::new(config, client_addr, server_addr))
                .with_upgrades()
                .await
            {
                println!("Failed to serve connection: {:?}", err);
            }
        });
    }
}
