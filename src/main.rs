use std::{fs, future::Future, net::SocketAddr, pin::Pin};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::{body::Incoming, client, header, server, service::Service, Request, Response};
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    target: SocketAddr,
    listen: SocketAddr,
}

struct Server {
    config: &'static Config,
}

struct Proxy {
    config: &'static Config,
}

impl Proxy {
    pub fn new(config: &'static Config) -> Self {
        Self { config }
    }
}

impl Service<Request<Incoming>> for Proxy {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;

    type Error = hyper::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        println!("{req:?}");
        let config = self.config;
        Box::pin(async move {
            let stream = TcpStream::connect(config.target).await.unwrap();

            let (mut sender, conn) = client::conn::http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .handshake(stream)
                .await?;

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    println!("Connection failed: {:?}", err);
                }
            });

            let mut res = sender.send_request(req).await?;

            println!("{res:?}");

            res.headers_mut()
                .insert(header::SERVER, header::HeaderValue::from_static("RXH"));

            Ok(res.map(|body| body.boxed()))
        })
    }
}

impl Server {
    pub fn new(config: &'static Config) -> Self {
        Self { config }
    }

    pub async fn listen(&self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.config.listen).await?;
        println!("Listening on http://{}", self.config.listen);
        let config = self.config;

        loop {
            let (stream, _) = listener.accept().await?;

            tokio::task::spawn(async move {
                if let Err(err) = server::conn::http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(stream, Proxy::new(config))
                    .with_upgrades()
                    .await
                {
                    println!("Failed to serve connection: {:?}", err);
                }
            });
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = serde_json::from_str(&fs::read_to_string("rxh.json")?)?;
    Server::new(Box::leak(config)).listen().await
}
