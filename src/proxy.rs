use std::{future::Future, net::SocketAddr, pin::Pin};

use http_body_util::BodyExt;
use hyper::{body::Incoming, service::Service, upgrade::Upgraded, Request};
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
/// both the client upgraded connection and the server upgraded connection. We
/// don't know when this connections are available since they're handled by
/// different tasks, so the have to be sent on a channel.
struct Tunnel {
    /// Used to receive the upgraded client IO when it's ready.
    client_upgraded_io_receiver: oneshot::Receiver<Upgraded>,

    /// Used to receive the upgraded backedn server IO when it's ready.
    server_upgraded_io_receiver: oneshot::Receiver<Upgraded>,
}

impl Tunnel {
    /// Inititalizes a new [`Tunnel`] which can be enabled later by calling
    /// [`Tunner::enable`].
    pub fn init() -> (Self, oneshot::Sender<Upgraded>, oneshot::Sender<Upgraded>) {
        let (client_tx, client_rx) = oneshot::channel();
        let (server_tx, server_rx) = oneshot::channel();

        let tunnel = Self {
            client_upgraded_io_receiver: client_rx,
            server_upgraded_io_receiver: server_rx,
        };

        (tunnel, client_tx, server_tx)
    }

    /// The tunnel waits until it receives the upgraded connections. Once both
    /// the client and server connections are ready, TCP traffic is forwarded
    /// from client to server and viceversa.
    pub async fn enable(self) {
        // TODO: Error handling
        let mut client_io = self.client_upgraded_io_receiver.await.unwrap();
        let mut server_io = self.server_upgraded_io_receiver.await.unwrap();

        let (client_bytes, server_bytes) =
            tokio::io::copy_bidirectional(&mut client_io, &mut server_io)
                .await
                .unwrap();

        println!("Client wrote {client_bytes}, server wrote {server_bytes}");
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

/// Forwards the request to the target server and returns the response sent
/// by the target server. See [`ProxyRequest`] and [`ProxyResponse`].
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

    let (tunnel, client_upgraded_io_sender, server_upgraded_io_sender) = Tunnel::init();

    if request.headers().contains_key(hyper::header::UPGRADE) {
        let (forward_request, upgrade_request) = request.into_upgraded();
        request = forward_request;

        tokio::task::spawn(async move {
            match hyper::upgrade::on(upgrade_request).await {
                Ok(upgraded) => client_upgraded_io_sender.send(upgraded).unwrap(),
                Err(err) => println!("err {err}"),
            };
        });
    }

    let mut response = ProxyResponse::new(sender.send_request(request.into_forwarded()).await?);

    if response.status() == http::StatusCode::SWITCHING_PROTOCOLS {
        let (forward_response, upgrade_response) = response.into_upgraded();
        response = forward_response;

        tokio::task::spawn(async move {
            match hyper::upgrade::on(upgrade_response).await {
                Ok(upgraded) => server_upgraded_io_sender.send(upgraded).unwrap(),
                Err(err) => println!("err {err}"),
            }
        });

        tokio::spawn(async move {
            tunnel.enable().await;
        });
    }

    Ok(response.into_forwarded().map(|body| body.boxed()))
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
