#![feature(associated_type_bounds)]
#![feature(trait_alias)]

use std::{convert::Infallible, future::Future, net::SocketAddr};

use bytes::Bytes;
use http::response::Parts;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Body, Incoming},
    service::{service_fn, Service},
    Request, Response,
};
use tokio::{
    self,
    io::AsyncWriteExt,
    net::{TcpListener, TcpSocket, TcpStream},
    task::JoinHandle,
};

/// Trait alias for request and response generic body bounds.
trait AsyncBody = Body<Data: Send, Error: Sync + Send + std::error::Error> + Send + 'static;

/// Serves HTTP connection using [`service_fn`].
async fn serve_connection<S, B>(stream: TcpStream, service: S)
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
fn spawn_backend_server<S, F, B>(addr: SocketAddr, service: S) -> JoinHandle<()>
where
    F: Future<Output = Result<Response<B>, Infallible>> + Send + 'static,
    S: FnMut(Request<Incoming>) -> F + Send + Copy + 'static,
    B: AsyncBody,
{
    tokio::task::spawn(async move {
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(stream, service_fn(service)).await;
        }
    })
}

/// Starts an RXH reverse proxy server in the background with the given config.
fn spawn_reverse_proxy(config: rxh::Config) -> JoinHandle<()> {
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
async fn send_http_request<B>(from: SocketAddr, to: SocketAddr, req: Request<B>) -> (Parts, Bytes)
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
// fn spawn_client<B>(from: SocketAddr, to: SocketAddr, req: Request<B>) -> JoinHandle<()>
// where
//     B: AsyncBody,
// {
//     tokio::task::spawn(async move {
//         ping_tcp_server(to).await;
//         send_http_request(from, to, req).await;
//     })
// }

/// Attempts to connect to a TCP server that's running as a Tokio task for a
/// number of retries. Each failed attempts yields the execution back to the
/// runtime, allowing Tokio to progress pending tasks. If all the attempts fail,
/// the function panicks and tests are stopped. This should work with both
/// single threaded runtime and multithreaded runtime.
async fn ping_tcp_server(addr: SocketAddr) {
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
async fn ping_all(addrs: &[SocketAddr]) {
    // This function is usually called after spawning the servers, so we can
    // yield right at the beginning and most likely the servers will already
    // be listening by the time we try to ping them.
    tokio::task::yield_now().await;
    for addr in addrs {
        ping_tcp_server(*addr).await;
    }
}

#[tokio::test]
async fn reverse_proxy_client() {
    let client_addr = "127.0.0.1:7000".parse().unwrap();
    let proxy_addr = "127.0.0.1:8000".parse().unwrap();
    let server_addr = "127.0.0.1:9000".parse().unwrap();

    spawn_backend_server(server_addr, |_| async {
        Ok(Response::new(Full::<Bytes>::from("Hello world")))
    });

    spawn_reverse_proxy(rxh::Config {
        listen: proxy_addr,
        target: server_addr,
        prefix: String::from("/"),
    });

    ping_all(&[server_addr, proxy_addr]).await;

    let (_, body) = send_http_request(client_addr, proxy_addr, request::empty()).await;

    assert_eq!(body, String::from("Hello world"));
}

#[tokio::test]
async fn reverse_proxy_backend() {
    // let client_addr = "127.0.0.1:7001".parse().unwrap();
    // let proxy_addr = "127.0.0.1:8001".parse().unwrap();
    // let server_addr = "127.0.0.1:9001".parse().unwrap();

    // TODO
}

mod request {
    //! Quick request factory.

    use bytes::Bytes;
    use http_body_util::Empty;
    use hyper::Request;

    pub fn empty() -> Request<Empty<Bytes>> {
        Request::builder().body(Empty::<Bytes>::new()).unwrap()
    }
}
