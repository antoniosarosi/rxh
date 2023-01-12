#![feature(associated_type_bounds)]
#![feature(trait_alias)]

mod util;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{header, service::service_fn, Response};
use tokio::{net::TcpListener, sync::mpsc};
use util::{
    ping_all, request, send_http_request, serve_connection, spawn_backend_server, spawn_client,
    spawn_reverse_proxy, BackendServer,
};

#[tokio::test]
async fn reverse_proxy_client() {
    let client_addr = "127.0.0.1:7000".parse().unwrap();
    let proxy_addr = "127.0.0.1:8000".parse().unwrap();
    let server_addr = "127.0.0.1:9000".parse().unwrap();

    spawn_backend_server(
        server_addr,
        service_fn(|_| async { Ok(Response::new(Full::<Bytes>::from("Hello world"))) }),
    );

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
    let client_addr = "127.0.0.1:7001".parse().unwrap();
    let proxy_addr = "127.0.0.1:8001".parse().unwrap();
    let server_addr = "127.0.0.1:9001".parse().unwrap();

    spawn_reverse_proxy(rxh::Config {
        listen: proxy_addr,
        target: server_addr,
        prefix: String::from("/"),
    });

    spawn_client(client_addr, proxy_addr, request::empty());

    let (tx, mut rx) = mpsc::channel(1);

    let listener = TcpListener::bind(server_addr).await.unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    serve_connection(stream, BackendServer::new(tx)).await;

    let (parts, _) = rx.recv().await.unwrap();
    let forwarded = parts
        .headers
        .get(header::FORWARDED)
        .unwrap()
        .to_str()
        .unwrap();

    assert_eq!(
        forwarded,
        format!("for={client_addr};by={proxy_addr};host={proxy_addr}")
    );
}
