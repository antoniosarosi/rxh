//! HTTP utilities for integrations tests.

use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{
    body::Incoming,
    client::conn::http1::SendRequest,
    service::{service_fn, Service},
    Request,
    Response,
};
use rxh::config::Backend;
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

/// Starts a new backend server in the background that counts the amount of
/// requests it receives. This is useful for testing load balancers.
pub fn spawn_backend_server_with_request_counter(weight: usize) -> (Backend, Arc<AtomicUsize>) {
    let request_counter = Arc::new(AtomicUsize::new(0));
    let owned_request_counter = request_counter.clone();

    let (listener, address) = usable_tcp_listener();
    let backend = Backend { address, weight };

    tokio::task::spawn(async move {
        loop {
            let request_counter = request_counter.clone();
            let service = service_fn(move |_| {
                request_counter.fetch_add(1, Ordering::Relaxed);
                async { Ok(Response::new(Empty::<Bytes>::new())) }
            });
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(stream, service).await;
        }
    });

    (backend, owned_request_counter)
}

/// Same as [`spawn_backend_server_with_request_counter`] but spawns multiple
/// backends.
pub fn spawn_backends_with_request_counters(
    weights: &[usize],
) -> (Vec<Backend>, Vec<Arc<AtomicUsize>>) {
    let mut backends = Vec::new();
    let mut request_counters = Vec::new();

    for weight in weights {
        let (backend, request_counter) = spawn_backend_server_with_request_counter(*weight);
        backends.push(backend);
        request_counters.push(request_counter);
    }

    (backends, request_counters)
}

/// Starts an RXH reverse proxy server in the background with the given config.
pub fn spawn_reverse_proxy(config: rxh::config::Server) -> (SocketAddr, JoinHandle<()>) {
    let server = rxh::Server::init(config).unwrap();

    let addr = server.socket_address();

    let handle = tokio::task::spawn(async {
        server.run().await.unwrap();
    });

    (addr, handle)
}

/// Starts an RXH reverse proxy server in the background with the given config
/// and provides access to shutdown trigger and state updates.
pub fn spawn_reverse_proxy_with_controllers(
    config: rxh::config::Server,
) -> (
    SocketAddr,
    JoinHandle<()>,
    impl FnOnce(),
    watch::Receiver<rxh::State>,
) {
    let (tx, rx) = oneshot::channel();

    let server = rxh::Server::init(config).unwrap().shutdown_on(rx);

    let addr = server.socket_address();
    let state = server.subscribe();

    let handle = tokio::task::spawn(async {
        server.run().await.unwrap();
    });

    (addr, handle, || tx.send(()).unwrap(), state)
}

/// Launches a master task in the background. TODO: Provide shutdown trigger
/// like [`spawn_reverse_proxy_with_controllers`].
pub fn spawn_master(config: rxh::config::NormalizedConfig) -> (Vec<SocketAddr>, JoinHandle<()>) {
    let master = rxh::Master::init(config).unwrap();
    let sockets = master.sockets();
    let handle = tokio::task::spawn(async move {
        master.run().await.unwrap();
    });

    (sockets, handle)
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

/// Sends an HTTP request from a random socket to the given address.
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
