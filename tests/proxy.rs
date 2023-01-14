#![feature(associated_type_bounds)]
#![feature(trait_alias)]

mod util;

use std::io;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{header, service::service_fn, Response};
use rxh::{Config, State};
use tokio::sync::mpsc;
use util::{
    http::{
        request,
        send_http_request,
        spawn_backend_server,
        spawn_client,
        spawn_reverse_proxy,
        spawn_reverse_proxy_with_controllers,
    },
    service::{serve_connection, RequestInterceptor},
    tcp::{ping_all, ping_tcp_server, usable_socket, usable_tcp_listener},
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

#[tokio::test]
async fn graceful_shutdown() {
    let (server_addr, _) = spawn_backend_server(service_fn(|_| async {
        Ok(Response::new(Full::<Bytes>::from("Hello world")))
    }));

    let (proxy_addr, _, shutdown, state) = spawn_reverse_proxy_with_controllers(Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: server_addr,
        prefix: String::from("/hello"),
    });

    ping_all(&[server_addr, proxy_addr]).await;

    assert_eq!(*state.borrow(), State::Listening);

    let (sock1, _) = usable_socket();
    let (sock2, _) = usable_socket();
    let (sock3, _) = usable_socket();

    // Open a couple sockets but don't send anything yet.
    let stream1 = sock1.connect(proxy_addr).await.unwrap();
    let stream2 = sock2.connect(proxy_addr).await.unwrap();

    // Yield execution back to Tokio so that the server task can run and accept
    // the previous connections.
    tokio::task::yield_now().await;

    // Shutdown the server.
    shutdown();

    // Yield again to let the server update it's internal state.
    tokio::task::yield_now().await;

    // Now the server should know that there are still 2 pending connections.
    assert_eq!(
        *state.borrow(),
        State::ShuttingDown(rxh::ShutdownState::PendingConnections(2))
    );

    // If we try to connect using another socket it should not allow us.
    assert_eq!(
        sock3.connect(proxy_addr).await.err().unwrap().kind(),
        io::ErrorKind::ConnectionRefused
    );

    for stream in [stream1, stream2] {
        // Send a simple HTTP request with the connected sockets.
        stream.writable().await.unwrap();
        assert!(stream.try_write(b"GET /hello HTTP/1.1\r\n\r\n").is_ok());

        // Read the response.
        stream.readable().await.unwrap();
        let mut buff = [0; 1024];
        let bytes = stream.try_read(buff.as_mut_slice()).unwrap();

        // Check that we've received an OK response with the body that we used
        // when spawning the backend server.
        assert!(buff.starts_with(b"HTTP/1.1 200 OK"));
        assert!(buff[..bytes].ends_with(b"Hello world"));
    }
}
