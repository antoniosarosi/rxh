use std::net::SocketAddr;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::{body::Incoming, header, service::service_fn, Request, Response};
use tokio::{
    self,
    net::{TcpListener, TcpSocket},
    sync::OnceCell,
};

#[tokio::test]
async fn reverse_proxy() {
    static CONFIG: OnceCell<rxh::Config> = OnceCell::const_new();

    let client_addr = SocketAddr::from(([127, 0, 0, 1], 9000));
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let proxy_addr = SocketAddr::from(([127, 0, 0, 1], 8100));
    let message: &'static str = "Hello World";

    let config = CONFIG
        .get_or_init(|| async {
            rxh::Config {
                listen: proxy_addr,
                target: server_addr,
                prefix: String::from("/"),
            }
        })
        .await;

    // Reverse proxy
    tokio::task::spawn(async { rxh::Server::new(config).listen().await.unwrap() });

    // Server behind reverse proxy
    tokio::task::spawn(async move {
        let listener = TcpListener::bind(server_addr).await.unwrap();

        let (stream, _) = listener.accept().await.unwrap();

        let service = |req: Request<Incoming>| async move {
            let Some(forwarded) = req.headers().get(header::FORWARDED) else {
                return Err("FORWARDED header not received");
            };

            assert_eq!(
                forwarded.to_str().unwrap(),
                format!("for={client_addr};by={proxy_addr};host={proxy_addr}")
            );

            Ok(Response::new(Full::<Bytes>::from(message)))
        };

        hyper::server::conn::http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .serve_connection(stream, service_fn(service))
            .await
            .unwrap()
    });

    // Client
    let client_handle = tokio::task::spawn(async move {
        let socket = TcpSocket::new_v4().unwrap();
        socket.bind(client_addr).unwrap();
        let stream = socket.connect(proxy_addr).await.unwrap();

        let (mut sender, conn) = hyper::client::conn::http1::handshake(stream).await.unwrap();
        tokio::task::spawn(async move { conn.await.unwrap() });

        let req = Request::builder()
            .header(hyper::header::HOST, proxy_addr.to_string())
            .body(Empty::<Bytes>::new())
            .unwrap();

        let res = sender.send_request(req).await.unwrap();
        let body = res.collect().await.unwrap().to_bytes();

        assert_eq!(body, String::from(message));
    });

    // Wait for the client to receive the response
    client_handle.await.unwrap();
}
