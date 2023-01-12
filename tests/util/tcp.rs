use std::net::SocketAddr;

use tokio::{
    self,
    io::AsyncWriteExt,
    net::{TcpListener, TcpSocket, TcpStream},
};

pub fn usable_socket() -> (TcpSocket, SocketAddr) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_reuseaddr(true).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = socket.local_addr().unwrap();

    (socket, addr)
}

pub fn usable_tcp_listener() -> (TcpListener, SocketAddr) {
    let (socket, _addr) = usable_socket();
    let listener = socket.listen(1024).unwrap();
    let addr = listener.local_addr().unwrap();

    (listener, addr)
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
