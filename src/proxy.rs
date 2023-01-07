use std::{future::Future, net::SocketAddr, pin::Pin};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::{body::Incoming, service::Service, Request, Response};
use tokio::net::TcpStream;

use crate::{config::Config, response};

/// Proxy service. Handles incoming requests from clients and responses from
/// target servers.
pub(crate) struct Proxy {
    /// Reference to global config.
    config: &'static Config,
}

impl Proxy {
    /// Creates a new [`Proxy`].
    pub fn new(config: &'static Config) -> Self {
        Self { config }
    }

    /// Forwards the request the target server and returns the response sent
    /// by the target server.
    pub async fn forward(
        request: Request<Incoming>,
        to: SocketAddr,
    ) -> Result<response::BoxBodyResponse, hyper::Error> {
        let stream = TcpStream::connect(to).await.unwrap();

        let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .handshake(stream)
            .await?;

        tokio::task::spawn(async move {
            if let Err(err) = conn.await {
                println!("Connection failed: {:?}", err);
            }
        });

        let response = sender.send_request(request).await?;

        Ok(response::annotate(response.map(|body| body.boxed())))
    }
}

impl Service<Request<Incoming>> for Proxy {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;

    type Error = hyper::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, request: Request<Incoming>) -> Self::Future {
        let config = self.config;
        Box::pin(async move {
            if !request.uri().to_string().starts_with(&config.prefix) {
                Ok(response::not_found())
            } else {
                Proxy::forward(request, config.target).await
            }
        })
    }
}
