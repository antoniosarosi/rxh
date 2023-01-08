use tokio::net::TcpListener;

use crate::{
    config::{Config, ConfigRef},
    proxy::Proxy,
};

/// TCP listener. Accepts new connections and spawns tasks to handle them.
pub(crate) struct Server<C> {
    /// Reference to global config.
    config: C,
}

impl<C> Server<C> {
    /// Creates a new [`Server`].
    pub fn new(config: C) -> Self {
        Self { config }
    }

    /// Starts listening for incoming connections on the address specified by
    /// [`self.config.listen`].
    pub async fn listen(&self) -> Result<(), Box<dyn std::error::Error>>
    where
        C: ConfigRef + Copy + Send + 'static,
    {
        let Self { config } = *self;
        let Config { listen, .. } = config.get();
        let listener = TcpListener::bind(listen).await?;
        println!("Listening on http://{}", listen);

        loop {
            let (stream, client_addr) = listener.accept().await?;
            // TODO: Unix domain Sockets
            let server_addr = stream.local_addr().unwrap();

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
}
