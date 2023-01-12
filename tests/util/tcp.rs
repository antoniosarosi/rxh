//! TCP utilities for integration tests.

use std::net::SocketAddr;

use tokio::{
    self,
    io::AsyncWriteExt,
    net::{TcpListener, TcpSocket, TcpStream},
};

/// Creates a socket binding it to port "0", which let's the OS pick any
/// available TCP port. This is useful because tests are run in parallel and
/// we don't want socket addresses to collide, but we still want to know
/// the socket address.
pub fn usable_socket() -> (TcpSocket, SocketAddr) {
    let socket = TcpSocket::new_v4().unwrap();

    #[cfg(not(windows))]
    socket.set_reuseaddr(true).unwrap();

    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = socket.local_addr().unwrap();

    (socket, addr)
}

/// Same as [`usable_socket`] but already configured for listening.
pub fn usable_tcp_listener() -> (TcpListener, SocketAddr) {
    let (socket, addr) = usable_socket();
    let listener = socket.listen(1024).unwrap();

    (listener, addr)
}

/// Attempts to connect to a TCP server that's running as a Tokio task for a
/// number of retries. Each failed attempts yields the execution back to the
/// runtime, allowing Tokio to progress pending tasks. If all the attempts fail,
/// the function panics and tests are stopped. This should work with both single
/// threaded runtime and multithreaded runtime.
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

/// Convinience for awaiting multiple servers. See [`ping_tcp_server`].
pub async fn ping_all(addrs: &[SocketAddr]) {
    // This function is usually called after spawning the servers, so we can
    // yield right at the beginning and most likely the servers will already
    // be listening by the time we try to ping them.
    tokio::task::yield_now().await;
    for addr in addrs {
        ping_tcp_server(*addr).await;
    }
}
