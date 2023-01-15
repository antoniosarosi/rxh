use std::{future::Future, net::SocketAddr, pin::Pin};

use http_body_util::BodyExt;
use hyper::{
    body::Incoming,
    service::Service,
    upgrade::{OnUpgrade, Upgraded},
    Request,
    Response,
};
use tokio::{net::TcpStream, sync::oneshot};

use crate::{
    config::Config,
    request::ProxyRequest,
    response::{self, BoxBodyResponse, LocalResponse, ProxyResponse},
};

/// Proxy service. Handles incoming requests from clients and responses from
/// target servers.
pub(crate) struct Proxy {
    /// Reference to global config.
    config: &'static Config,
    client_addr: SocketAddr,
    server_addr: SocketAddr,
}

struct Tunnel {
    client_rx: oneshot::Receiver<Upgraded>,
    server_rx: oneshot::Receiver<Upgraded>,
}

impl Tunnel {
    pub fn new(
        client_rx: oneshot::Receiver<Upgraded>,
        server_rx: oneshot::Receiver<Upgraded>,
    ) -> Self {
        Self {
            client_rx,
            server_rx,
        }
    }

    // TODO: Error handling
    pub async fn allow(self) {
        let mut client = self.client_rx.await.unwrap();
        let mut server = self.server_rx.await.unwrap();

        let (c, s) = tokio::io::copy_bidirectional(&mut client, &mut server)
            .await
            .unwrap();

        println!("Client wrote {c} bytes, server wrote {s} bytes");
    }
}

impl Proxy {
    /// Creates a new [`Proxy`].
    pub fn new(config: &'static Config, client_addr: SocketAddr, server_addr: SocketAddr) -> Self {
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

    let mut request = request.into_forwarded();

    let (client_tx, client_rx) = oneshot::channel();
    let (server_tx, server_rx) = oneshot::channel();

    let tunnel = Tunnel::new(client_rx, server_rx);

    if request.headers().contains_key(hyper::header::UPGRADE) {
        let (parts, body) = request.into_parts();
        let mut builder = Request::builder()
            .method(&parts.method)
            .uri(&parts.uri)
            .version(parts.version.clone());
        *builder.headers_mut().unwrap() = parts.headers.clone();

        request = builder.body(body).unwrap();

        let mut builder = Request::builder()
            .method(&parts.method)
            .uri(&parts.uri)
            .version(parts.version.clone());
        *builder.headers_mut().unwrap() = parts.headers;
        *builder.extensions_mut().unwrap() = parts.extensions;
        let upgrade_request = builder.body(response::body::empty()).unwrap();

        tokio::task::spawn(async move {
            match hyper::upgrade::on(upgrade_request).await {
                Ok(upgraded) => {
                    client_tx.send(upgraded).unwrap();
                }
                Err(err) => println!("err {err}"),
            };
        });
    }

    let mut response = sender.send_request(request).await?;

    if response.status() == http::StatusCode::SWITCHING_PROTOCOLS {
        let (parts, body) = response.into_parts();

        let mut builder = Response::builder()
            .status(parts.status)
            .version(parts.version);
        *builder.headers_mut().unwrap() = parts.headers.clone();

        response = builder.body(body).unwrap();

        let mut builder = Response::builder()
            .status(parts.status)
            .version(parts.version);
        *builder.headers_mut().unwrap() = parts.headers;
        *builder.extensions_mut().unwrap() = parts.extensions;

        let upgrade_response = builder.body(response::body::empty()).unwrap();

        tokio::task::spawn(async move {
            match hyper::upgrade::on(upgrade_response).await {
                Ok(upgraded) => {
                    server_tx.send(upgraded).unwrap();
                }
                Err(err) => println!("err {err}"),
            }
        });

        tokio::spawn(async move {
            tunnel.allow().await;
        });
    }

    Ok(ProxyResponse::new(response.map(|body| body.boxed())).into_forwarded())
}

impl Service<Request<Incoming>> for Proxy {
    type Response = BoxBodyResponse;

    type Error = hyper::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, request: Request<Incoming>) -> Self::Future {
        let Proxy {
            client_addr,
            server_addr,
            config,
            ..
        } = *self;

        // Avoid cloning. Unwrapping is ok because we've only called this
        // function once. Subsequent calls would return an already taken error.

        Box::pin(async move {
            if !request.uri().to_string().starts_with(&config.prefix) {
                Ok(LocalResponse::not_found())
            } else {
                let request = ProxyRequest::new(request, client_addr, server_addr);
                proxy_forward(request, config.target).await
            }
        })
    }
}
