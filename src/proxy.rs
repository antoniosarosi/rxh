use std::{future::Future, net::SocketAddr, pin::Pin};

use http_body_util::BodyExt;
use hyper::{body::Incoming, header, service::Service, upgrade::Upgraded, Request, Response};
use tokio::{net::TcpStream, sync::oneshot};

use crate::{
    config::Config,
    request::ProxyRequest,
    response::{BoxBodyResponse, LocalResponse, ProxyResponse},
};

/// Proxy service. Handles incoming requests from clients and responses from
/// target servers.
pub(crate) struct Proxy {
    /// Reference to global config.
    config: &'static Config,

    // Socket address of the connected client.
    client_addr: SocketAddr,

    // Listening socket address.
    server_addr: SocketAddr,
}

/// TCP tunnel for upgraded connections such as Websockets. This tunnel needs
/// both the client [`Upgraded`] connection and the server [`Upgraded`]
/// connection. We can only obtain the client upgraded connection once we send
/// an `HTTP 101` response, so we'll receive that through a [`oneshot`] channel.
struct Tunnel {
    /// When the client [`Upgraded`] connection is ready, we'll receive it here.
    client_io_receiver: oneshot::Receiver<Upgraded>,
}

impl Tunnel {
    /// Opens a new [`Tunnel`] which can be sealed later using [`Tunnel::seal`].
    pub fn open(client_io_receiver: oneshot::Receiver<Upgraded>) -> Self {
        Self { client_io_receiver }
    }

    /// Sealing a [`Tunnel`] allows data to flow in both directions once the
    /// client [`Upgraded`] connection is received. This should be called inside
    /// a spawned [`tokio::task`] as it will wait on the [`oneshot`] channel
    /// until client upgraded IO is ready, which only happens after we respond
    /// back to the client.
    pub async fn seal(self, mut server: Upgraded) {
        let mut client = self.client_io_receiver.await.unwrap();
        match tokio::io::copy_bidirectional(&mut client, &mut server).await {
            Ok((client_bytes, server_bytes)) => {
                println!("Client wrote {client_bytes} bytes, server wrote {server_bytes} bytes")
            }
            Err(err) => eprintln!("Tunnel error: {err}"),
        }
    }
}

impl Proxy {
    /// Creates a new [`Proxy`] service.
    pub fn new(config: &'static Config, client_addr: SocketAddr, server_addr: SocketAddr) -> Self {
        Self {
            config,
            client_addr,
            server_addr,
        }
    }
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
        } = *self;

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

/// Forwards the request to the target server and returns the response sent
/// by the target server. See [`ProxyRequest`] and [`ProxyResponse`]. If the
/// client wants to upgrade the connection and the server agrees by sending
/// a `101` status code, then a TCP tunnel that forwards traffic bidirectionally
/// is spawned in a new Tokio task. Upgrading is a little bit tricky, see
/// [`ProxyRequest::into_upgraded`] method.
async fn proxy_forward(
    mut request: ProxyRequest<Incoming>,
    to: SocketAddr,
) -> Result<BoxBodyResponse, hyper::Error> {
    let Ok(stream) = TcpStream::connect(to).await else {
        return Ok(LocalResponse::bad_gateway());
    };

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

    let mut maybe_tunnel = None;

    if request.headers().contains_key(header::UPGRADE) {
        let (client_io_sender, client_io_receiver) = oneshot::channel();
        maybe_tunnel = Some(Tunnel::open(client_io_receiver));
        request = upgrade_client(request, client_io_sender);
    }

    let mut response = sender.send_request(request.into_forwarded()).await?;

    if response.status() == http::StatusCode::SWITCHING_PROTOCOLS {
        match maybe_tunnel {
            Some(tunnel) => upgrade_server(&mut response, tunnel).await,
            None => return Ok(LocalResponse::bad_gateway()),
        }
    }

    Ok(ProxyResponse::new(response.map(|body| body.boxed())).into_forwarded())
}

/// See [`ProxyRequest::into_upgraded`] for an explanation of how upgrades work
/// on the client side.
fn upgrade_client(
    request: ProxyRequest<Incoming>,
    client_io_sender: oneshot::Sender<Upgraded>,
) -> ProxyRequest<Incoming> {
    let (forward_request, upgrade_request) = request.into_upgraded();

    tokio::task::spawn(async move {
        match hyper::upgrade::on(upgrade_request).await {
            Ok(upgraded) => client_io_sender.send(upgraded).unwrap(),
            Err(err) => eprintln!("Error upgrading connection {err}"),
        };
    });

    forward_request
}

/// The upstream server connection can only be upgraded if there is an open
/// tunnel where data can pass through. In this case, it's not necessary to
/// spawn a [`tokio::task`] because [`hyper`] will upgrade immediately once it
/// sees the `101` status on the response, which makes things much easier since
/// we don't have to give up ownership on the request.
async fn upgrade_server(response: &mut Response<Incoming>, tunnel: Tunnel) {
    match hyper::upgrade::on(response).await {
        Ok(upgraded) => {
            tokio::task::spawn(tunnel.seal(upgraded));
        }
        Err(err) => eprintln!("Error upgrading connection {err}"),
    };
}
