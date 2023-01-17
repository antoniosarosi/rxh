use std::net::SocketAddr;

use http_body_util::BodyExt;
use hyper::{body::Incoming, header, upgrade::OnUpgrade};
use tokio::net::TcpStream;

use crate::http::{
    request::ProxyRequest,
    response::{BoxBodyResponse, LocalResponse, ProxyResponse},
};

/// Forwards the request to the target server and returns the response sent
/// by the target server. See [`ProxyRequest`] and [`ProxyResponse`]. If the
/// client wants to upgrade the connection and the server agrees by sending
/// a `101` status code, then a TCP tunnel that forwards traffic bidirectionally
/// is spawned in a new Tokio task.
pub(super) async fn forward(
    mut request: ProxyRequest<Incoming>,
    to: SocketAddr,
) -> Result<BoxBodyResponse, hyper::Error> {
    let Ok(stream) = TcpStream::connect(to).await else {
        return Ok(LocalResponse::bad_gateway());
    };

    let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .handshake(stream)
        .await?;

    tokio::task::spawn(async move {
        if let Err(err) = conn.await {
            println!("Connection failed: {:?}", err);
        }
    });

    let mut maybe_client_upgrade = None;

    if request.headers().contains_key(header::UPGRADE) {
        let upgrade = request.extensions_mut().remove::<OnUpgrade>().unwrap();
        maybe_client_upgrade = Some(upgrade);
    }

    let mut response = sender.send_request(request.into_forwarded()).await?;

    if response.status() == http::StatusCode::SWITCHING_PROTOCOLS {
        if let Some(client_upgrade) = maybe_client_upgrade {
            let server_upgrade = response.extensions_mut().remove::<OnUpgrade>().unwrap();
            tokio::task::spawn(tunnel(client_upgrade, server_upgrade));
        } else {
            // Upstream server sent us an HTTP 101 response without the client
            // asking for an upgrade, so we can't proxy data from the client.
            return Ok(LocalResponse::bad_gateway());
        }
    }

    Ok(ProxyResponse::new(response.map(|body| body.boxed())).into_forwarded())
}

/// TCP tunnel for upgraded connections such as Websockets or any other custom
/// protocol. This future should be spawned in a [`tokio::task`] as the client
/// [`hyper::upgrade::Upgraded`] connection won't resolve until we send an
/// `HTTP 101` response back to the client.
async fn tunnel(client: OnUpgrade, server: OnUpgrade) {
    let (mut upgraded_client, mut upgraded_server) = tokio::try_join!(client, server).unwrap();

    match tokio::io::copy_bidirectional(&mut upgraded_client, &mut upgraded_server).await {
        Ok((client_bytes, server_bytes)) => {
            println!("Client wrote {client_bytes} bytes, server wrote {server_bytes} bytes")
        }
        Err(err) => eprintln!("Tunnel error: {err}"),
    }
}
