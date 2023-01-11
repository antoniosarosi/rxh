use std::{future::Future, ptr};

use tokio::net::TcpListener;

use crate::{config::Config, proxy::Proxy};

/// Spawns a new server configured according to `config` parameter and
/// gracefully shuts down the server when the `shoutdown` future is ready.
pub async fn start(
    config: Config,
    shutdown: impl Future,
) -> Result<(), Box<dyn std::error::Error>> {
    // Leak the configuration to get a 'static lifetime, which we need to
    // spawn tokio tasks. Later when all tasks have finished, we'll drop this
    // value to avoid actual memory leaks.
    let config = Box::leak(Box::new(config));

    // let mut listen_result = Ok(());

    tokio::select! {
        _result = listen(config) => {
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

/// Starts listening for incoming connections on the address
async fn listen(config: &'static Config) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(config.listen).await?;
    println!("Listening on http://{}", config.listen);

    loop {
        let (stream, client_addr) = listener.accept().await?;
        // TODO: Unix domain Sockets
        let server_addr = stream.local_addr().unwrap();

        let config: &'static Config = config;

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
