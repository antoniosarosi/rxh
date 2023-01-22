//! RXH proxy integration tests.

#![feature(associated_type_bounds)]
#![feature(trait_alias)]

mod util;

use std::{io, sync::atomic::Ordering};

use bytes::Bytes;
use http::HeaderValue;
use http_body_util::{Empty, Full};
use hyper::{header, service::service_fn, Request, Response};
use rxh::{ShutdownState, State};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

use crate::util::{
    config,
    http::{
        http_client,
        request,
        send_http_request,
        spawn_backend_server,
        spawn_backends_with_request_counters,
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

    let (proxy_addr, _) = spawn_reverse_proxy(config::proxy::single_backend(server_addr));

    ping_all(&[server_addr, proxy_addr]).await;

    let (_, body) = send_http_request(proxy_addr, request::empty()).await;

    assert_eq!(body, String::from("Hello world"));
}

#[tokio::test]
async fn reverse_proxy_client_receives_404_on_bad_prefix() {
    let (proxy_addr, _) = spawn_reverse_proxy(config::proxy::single_backend_with_uri(
        "127.0.0.1:0".parse().unwrap(),
        "/prefix",
    ));

    ping_tcp_server(proxy_addr).await;

    let uris = ["/unknown", "/invalid", "/wrong", "/test/longer"];

    for uri in uris {
        let (parts, _) = send_http_request(proxy_addr, request::empty_with_uri(uri)).await;
        assert_eq!(parts.status, http::StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn reverse_proxy_client_receives_502_on_backend_server_not_available() {
    let (_, server_socket_addr) = usable_socket();

    let (proxy_addr, _) = spawn_reverse_proxy(config::proxy::single_backend(server_socket_addr));

    ping_tcp_server(proxy_addr).await;

    let (parts, _) = send_http_request(proxy_addr, request::empty()).await;

    assert_eq!(parts.status, http::StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn reverse_proxy_backend() {
    let (listener, server_addr) = usable_tcp_listener();

    let (proxy_addr, _) = spawn_reverse_proxy(config::proxy::single_backend(server_addr));

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

    let (proxy_addr, _, shutdown, mut state) = spawn_reverse_proxy_with_controllers(
        config::proxy::single_backend_with_uri(server_addr, "/hello"),
    );

    ping_all(&[server_addr, proxy_addr]).await;

    // Make sure server is listening.
    state.changed().await.unwrap();
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

    // Wait for the state change.
    state.changed().await.unwrap();

    // Now the server should know that there are still 2 pending connections.
    assert_eq!(
        *state.borrow(),
        State::ShuttingDown(ShutdownState::PendingConnections(2))
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

    // Finally, after the streams above are dropped, server should be down.
    state.changed().await.unwrap();
    assert_eq!(*state.borrow(), State::ShuttingDown(ShutdownState::Done));
}

#[tokio::test]
async fn upgraded_connection() {
    let (server_addr, _) = spawn_backend_server(service_fn(|req| async {
        tokio::task::spawn(async move {
            let mut upgraded = hyper::upgrade::on(req).await.unwrap();
            let mut buff = [0; 1024];
            let bytes = upgraded.read(&mut buff).await.unwrap();
            upgraded.write_all(&buff[0..bytes]).await.unwrap();
            upgraded.shutdown().await.unwrap();
        });

        Ok(Response::builder()
            .status(http::StatusCode::SWITCHING_PROTOCOLS)
            .header(header::UPGRADE, HeaderValue::from_static("testproto"))
            .body(Empty::<Bytes>::new())
            .unwrap())
    }));

    let (proxy_addr, _) = spawn_reverse_proxy(config::proxy::single_backend(server_addr));

    ping_all(&[server_addr, proxy_addr]).await;

    let (socket, _) = usable_socket();
    let stream = socket.connect(proxy_addr).await.unwrap();
    let mut sender = http_client(stream).await;

    let req = Request::builder()
        .header(header::CONNECTION, HeaderValue::from_static("upgrade"))
        .header(header::UPGRADE, HeaderValue::from_static("testproto"))
        .body(Empty::<Bytes>::new())
        .unwrap();

    let res = sender.send_request(req).await.unwrap();

    let mut upgraded = hyper::upgrade::on(res).await.unwrap();
    upgraded.write(b"Test String").await.unwrap();

    let mut buff = [0; 1024];
    let bytes = upgraded.read(&mut buff).await.unwrap();
    upgraded.shutdown().await.unwrap();

    assert_eq!(&buff[0..bytes], b"Test String");
}

#[tokio::test]
async fn load_balancing() {
    let weights = vec![1, 3, 2];
    let (backends, request_counters) = spawn_backends_with_request_counters(&weights);

    let servers: Vec<_> = backends.iter().map(|backend| backend.address).collect();
    let (proxy, _) = spawn_reverse_proxy(config::proxy::multiple_weighted_backends(backends));

    ping_all(&servers).await;
    ping_tcp_server(proxy).await;

    let cycles = 10;

    for cycle in 1..=cycles {
        // Send a burst of requests at once.
        for _ in 0..weights.iter().sum() {
            send_http_request(proxy, request::empty()).await;
        }

        // Check that each backend server has received a number of requests that
        // matches its weight.
        for (num, weight) in request_counters.iter().zip(&weights) {
            assert_eq!(num.load(Ordering::Relaxed), weight * cycle);
        }
    }
}

#[tokio::test]
async fn serve_files() {
    let html = r#"
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta http-equiv="X-UA-Compatible" content="IE=edge">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Test</title>
        </head>
        <body>
            <p>Hello World</p>
        </body>
        </html>
    "#;

    let dir = tempfile::tempdir().unwrap();
    let mut file = tokio::fs::File::create(dir.path().join("index.html"))
        .await
        .unwrap();
    file.write(html.as_bytes()).await.unwrap();

    let (addr, _) = spawn_reverse_proxy(config::files::serve(dir.path().to_str().unwrap()));

    ping_tcp_server(addr).await;

    let (parts, body) = send_http_request(addr, request::empty_with_uri("/index.html")).await;

    assert_eq!(parts.status, http::StatusCode::OK);
    assert_eq!(body, html.as_bytes());
}
