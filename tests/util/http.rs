//! HTTP utilities for integrations tests.

use std::{convert::Infallible, net::SocketAddr};

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{
    body::Incoming,
    client::conn::http1::SendRequest,
    service::Service,
    Request,
    Response,
};
use tokio::{
    self,
    net::{TcpSocket, TcpStream},
    sync::{oneshot, watch},
    task::JoinHandle,
};

use super::{
    service::{serve_connection, AsyncBody},
    tcp::{ping_tcp_server, usable_socket, usable_tcp_listener},
};

/// Starts a backend server in the background with a customizable request
/// handler, returning the listening address and task handle.
pub fn spawn_backend_server<S, B>(service: S) -> (SocketAddr, JoinHandle<()>)
where
    S: Service<Request<Incoming>, Response = Response<B>, Error = Infallible, Future: Send>
        + Send
        + Copy
        + 'static,
    B: AsyncBody,
{
    let (listener, addr) = usable_tcp_listener();

    let handle = tokio::task::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(stream, service).await;
        }
    });

    (addr, handle)
}

/// Starts an RXH reverse proxy server in the background with the given config.
pub fn spawn_reverse_proxy(config: rxh::config::Server) -> (SocketAddr, JoinHandle<()>) {
    let server = rxh::Server::init(config, 0).unwrap();

    let addr = server.socket_address();

    let handle = tokio::task::spawn(async {
        server.run().await.unwrap();
    });

    (addr, handle)
}

/// Starts an RXH reverse proxy server in the background with the given config.
pub fn spawn_reverse_proxy_with_controllers(
    config: rxh::config::Server,
) -> (
    SocketAddr,
    JoinHandle<()>,
    impl FnOnce(),
    watch::Receiver<rxh::State>,
) {
    let (tx, rx) = oneshot::channel();

    let server = rxh::Server::init(config, 0).unwrap().shutdown_on(rx);

    let addr = server.socket_address();
    let state = server.subscribe();

    let handle = tokio::task::spawn(async {
        server.run().await.unwrap();
    });

    (addr, handle, || tx.send(()).unwrap(), state)
}

/// Provides an HTTP client that spawns a connection object in the background
/// to manage request transmissions.
pub async fn http_client<B: AsyncBody>(stream: TcpStream) -> SendRequest<B> {
    let (sender, conn) = hyper::client::conn::http1::handshake(stream).await.unwrap();
    tokio::task::spawn(async move { conn.await.unwrap() });

    sender
}

/// Sends an HTTP request from the given [`TcpSocket`] to the given
/// [`SocketAddr`].
pub async fn send_http_request_from<B>(
    from: TcpSocket,
    to: SocketAddr,
    req: Request<B>,
) -> (http::response::Parts, Bytes)
where
    B: AsyncBody,
{
    let stream = from.connect(to).await.unwrap();
    let mut sender = http_client(stream).await;

    let (parts, body) = sender.send_request(req).await.unwrap().into_parts();
    (parts, body.collect().await.unwrap().to_bytes())
}

pub async fn send_http_request<B>(to: SocketAddr, req: Request<B>) -> (http::response::Parts, Bytes)
where
    B: AsyncBody,
{
    send_http_request_from(usable_socket().0, to, req).await
}

/// Same as [`send_http_request_from`] but runs as a different task. This allows
/// the current task to continue execution.
pub fn spawn_client<B>(target: SocketAddr, req: Request<B>) -> (SocketAddr, JoinHandle<()>)
where
    B: AsyncBody,
{
    let (socket, addr) = usable_socket();

    let handle = tokio::task::spawn(async move {
        ping_tcp_server(target).await;
        send_http_request_from(socket, target, req).await;
    });

    (addr, handle)
}

pub mod request {
    //! Quick request factory.

    use bytes::Bytes;
    use http_body_util::Empty;
    use hyper::Request;

    pub fn empty() -> Request<Empty<Bytes>> {
        Request::builder().body(Empty::<Bytes>::new()).unwrap()
    }

    pub fn empty_with_uri(uri: &str) -> Request<Empty<Bytes>> {
        Request::builder()
            .uri(uri)
            .body(Empty::<Bytes>::new())
            .unwrap()
    }
}
