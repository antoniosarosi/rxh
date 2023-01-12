//! Some nice utilities for writing automated tests for servers and reverse
//! proxies running on the same tokio runtime.

use std::{convert::Infallible, future::Future, net::SocketAddr, pin::Pin};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Body, Incoming},
    service::Service,
    Request, Response,
};
use tokio::{
    self,
    io::AsyncWriteExt,
    net::{TcpListener, TcpSocket, TcpStream},
    sync::mpsc,
    task::JoinHandle,
};

/// Backend server that can run on different tasks and shares every request that
/// it receives on a channel. This allows us to write cleaner tests where all
/// asserts are done in the test function, not on a separate task.
pub struct BackendServer {
    tx: mpsc::Sender<(http::request::Parts, Bytes)>,
}

impl BackendServer {
    pub fn new(tx: mpsc::Sender<(http::request::Parts, Bytes)>) -> Self {
        Self { tx }
    }
}

impl Service<Request<Incoming>> for BackendServer {
    type Response = Response<Full<Bytes>>;

    type Error = Infallible;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let tx = self.tx.clone();
        Box::pin(async move {
            let (parts, body) = req.into_parts();

            tx.send((parts, body.collect().await.unwrap().to_bytes()))
                .await
                .unwrap();

            Ok(Response::new(Full::<Bytes>::from("Hello world")))
        })
    }
}

/// Trait alias for request and response generic body bounds.
pub trait AsyncBody = Body<Data: Send, Error: Sync + Send + std::error::Error> + Send + 'static;

/// Serves HTTP connection using [`service_fn`].
pub async fn serve_connection<S, B>(stream: TcpStream, service: S)
where
    S: Service<Request<Incoming>, Response = Response<B>, Error = Infallible>,
    B: AsyncBody,
{
    hyper::server::conn::http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(stream, service)
        .await
        .unwrap();
}

/// Starts a backend server in the background with a customizable request
/// handler.
pub fn spawn_backend_server<S, B>(addr: SocketAddr, service: S) -> JoinHandle<()>
where
    S: Service<Request<Incoming>, Response = Response<B>, Error = Infallible, Future: Send>
        + Send
        + Copy
        + 'static,
    B: AsyncBody,
{
    tokio::task::spawn(async move {
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(stream, service).await;
        }
    })
}

/// Starts an RXH reverse proxy server in the background with the given config.
pub fn spawn_reverse_proxy(config: rxh::Config) -> JoinHandle<()> {
    tokio::task::spawn(async {
        // TODO: Replace CTRL-C with something we can actually controll.
        rxh::server::start(config, tokio::signal::ctrl_c())
            .await
            .unwrap()
    })
}

/// Opens a socket and binds it to `from` address before sending an HTTP request
/// to `to` address. When the response is completely received including the
/// whole body, its parts are returned.
pub async fn send_http_request<B>(
    from: SocketAddr,
    to: SocketAddr,
    req: Request<B>,
) -> (http::response::Parts, Bytes)
where
    B: AsyncBody,
{
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind(from).unwrap();
    let stream = socket.connect(to).await.unwrap();

    let (mut sender, conn) = hyper::client::conn::http1::handshake(stream).await.unwrap();
    tokio::task::spawn(async move { conn.await.unwrap() });

    let (parts, body) = sender.send_request(req).await.unwrap().into_parts();

    (parts, body.collect().await.unwrap().to_bytes())
}

/// Same as [`send_http_request`] but from another task. This allows the
/// current task to continue execution.
pub fn spawn_client<B>(from: SocketAddr, to: SocketAddr, req: Request<B>) -> JoinHandle<()>
where
    B: AsyncBody,
{
    tokio::task::spawn(async move {
        ping_tcp_server(to).await;
        send_http_request(from, to, req).await;
    })
}

/// Attempts to connect to a TCP server that's running as a Tokio task for a
/// number of retries. Each failed attempts yields the execution back to the
/// runtime, allowing Tokio to progress pending tasks. If all the attempts fail,
/// the function panicks and tests are stopped. This should work with both
/// single threaded runtime and multithreaded runtime.
pub async fn ping_tcp_server(addr: SocketAddr) {
    let retries = 10;

    for _ in 0..retries {
        match TcpStream::connect(addr).await {
            Ok(mut stream) => {
                stream.shutdown().await.unwrap();
                return;
            }
            Err(_) => tokio::task::yield_now().await,
        }
    }

    panic!("Could not connect to server {addr}");
}

/// Convinience for awaiting multiple servers.
pub async fn ping_all(addrs: &[SocketAddr]) {
    // This function is usually called after spawning the servers, so we can
    // yield right at the beginning and most likely the servers will already
    // be listening by the time we try to ping them.
    tokio::task::yield_now().await;
    for addr in addrs {
        ping_tcp_server(*addr).await;
    }
}

pub mod request {
    //! Quick request factory.

    use bytes::Bytes;
    use http_body_util::Empty;
    use hyper::Request;

    pub fn empty() -> Request<Empty<Bytes>> {
        Request::builder().body(Empty::<Bytes>::new()).unwrap()
    }
}
