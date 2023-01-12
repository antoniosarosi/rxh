#![feature(associated_type_bounds)]
#![feature(trait_alias)]

mod util;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{header, service::service_fn, Response};
use rxh::Config;
use tokio::sync::mpsc;
use util::{
    http::{request, send_http_request, spawn_backend_server, spawn_client, spawn_reverse_proxy},
    service::{serve_connection, RequestInterceptor},
    tcp::{ping_all, ping_tcp_server, usable_tcp_listener},
};

#[tokio::test]
async fn reverse_proxy_client() {
    let (server_addr, _) = spawn_backend_server(service_fn(|_| async {
        Ok(Response::new(Full::<Bytes>::from("Hello world")))
    }));

    let (proxy_addr, _) = spawn_reverse_proxy(Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: server_addr,
        prefix: String::from("/"),
    });

    ping_all(&[server_addr, proxy_addr]).await;

    let (_, body) = send_http_request(proxy_addr, request::empty()).await;

    assert_eq!(body, String::from("Hello world"));
}

#[tokio::test]
async fn reverse_proxy_client_receives_404_on_bad_prefix() {
    let (proxy_addr, _) = spawn_reverse_proxy(Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: "127.0.0.1:8080".parse().unwrap(),
        prefix: String::from("/prefix"),
    });

    ping_tcp_server(proxy_addr).await;

    let uris = ["/unknown", "/invalid", "/wrong", "/test/longer"];

    for uri in uris {
        let (parts, _) = send_http_request(proxy_addr, request::empty_with_uri(uri)).await;
        assert_eq!(parts.status, 404);
    }
}

#[tokio::test]
async fn reverse_proxy_backend() {
    let (listener, server_addr) = usable_tcp_listener();

    let (proxy_addr, _) = spawn_reverse_proxy(Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: server_addr,
        prefix: String::from("/"),
    });

    let (client_addr, _) = spawn_client(proxy_addr, request::empty());

    let (tx, mut rx) = mpsc::channel(1);

    let (stream, _) = listener.accept().await.unwrap();
    serve_connection(stream, RequestInterceptor::new(tx)).await;

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
