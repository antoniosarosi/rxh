use tokio::net::TcpListener;

use crate::{config::Config, proxy::Proxy};

/// TCP listener. Accepts new connections and spawns tasks to handle them.
pub(crate) struct Server {
    /// Reference to global config.
    config: &'static Config,
}

impl Server {
    /// Creates a new [`Server`].
    pub fn new(config: &'static Config) -> Self {
        Self { config }
    }

    /// Starts listening for incoming connections on the address specified by
    /// [`self.config.listen`].
    pub async fn listen(&self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.config.listen).await?;
        println!("Listening on http://{}", self.config.listen);
        let config = self.config;

        loop {
            let (stream, _) = listener.accept().await?;

            tokio::task::spawn(async move {
                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(stream, Proxy::new(config))
                    .with_upgrades()
                    .await
                {
                    println!("Failed to serve connection: {:?}", err);
                }
            });
        }
    }
}
