use std::{future::Future, net::SocketAddr, pin::Pin};

use http_body_util::BodyExt;
use hyper::{body::Incoming, service::Service, Request};
use tokio::net::TcpStream;

use crate::{
    config::{Config, ConfigRef},
    request::ProxyRequest,
    response::{BoxBodyResponse, LocalResponse, ProxyResponse},
};

/// Proxy service. Handles incoming requests from clients and responses from
/// target servers.
pub(crate) struct Proxy<C> {
    /// Reference to global config.
    config: C,
    client_addr: SocketAddr,
    server_addr: SocketAddr,
}

impl<C> Proxy<C> {
    /// Creates a new [`Proxy`].
    pub fn new(config: C, client_addr: SocketAddr, server_addr: SocketAddr) -> Self {
        Self {
            config,
            client_addr,
            server_addr,
        }
    }
}

/// Forwards the request to the target server and returns the response sent
/// by the target server. See [`ProxyRequest`] and [`ProxyResponse`].
async fn proxy_forward(
    request: ProxyRequest<Incoming>,
    to: SocketAddr,
) -> Result<BoxBodyResponse, hyper::Error> {
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

    let response = sender.send_request(request.into_forwarded()).await?;

    Ok(ProxyResponse::new(response.map(|body| body.boxed())).into_forwarded())
}

impl<C> Service<Request<Incoming>> for Proxy<C>
where
    C: ConfigRef + Send + Copy + 'static,
{
    type Response = BoxBodyResponse;

    type Error = hyper::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, request: Request<Incoming>) -> Self::Future {
        let Proxy {
            client_addr,
            server_addr,
            config,
        } = *self;

        Box::pin(async move {
            let Config { prefix, target, .. } = config.get();

            if !request.uri().to_string().starts_with(prefix) {
                Ok(LocalResponse::not_found())
            } else {
                let request = ProxyRequest::new(request, client_addr, server_addr);
                proxy_forward(request, *target).await
            }
        })
    }
}
