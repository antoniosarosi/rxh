//! Wrappers and abstractions over [`hyper`] HTTP services.

use std::{convert::Infallible, future::Future, pin::Pin};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Body, Incoming},
    service::Service,
    Request, Response,
};
use tokio::{self, net::TcpStream, sync::mpsc};

/// Backend server that can run on different tasks and shares every request that
/// it receives on a channel. This allows us to write cleaner tests where all
/// asserts are done in the test function, not on a separate task.
pub struct RequestInterceptor {
    tx: mpsc::Sender<(http::request::Parts, Bytes)>,
}

impl RequestInterceptor {
    pub fn new(tx: mpsc::Sender<(http::request::Parts, Bytes)>) -> Self {
        Self { tx }
    }
}

impl Service<Request<Incoming>> for RequestInterceptor {
    type Response = Response<Full<Bytes>>;

    type Error = Infallible;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let tx = self.tx.clone();
        Box::pin(async move {
            let (parts, body) = req.into_parts();

            tx.send((parts, body.collect().await.unwrap().to_bytes()))
                .await
                .unwrap();

            Ok(Response::new(Full::<Bytes>::from("Hello world")))
        })
    }
}

/// Trait alias for request and response generic body bounds.
pub trait AsyncBody = Body<Data: Send, Error: Sync + Send + std::error::Error> + Send + 'static;

/// Serves HTTP connection using [`service_fn`].
pub async fn serve_connection<S, B>(stream: TcpStream, service: S)
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
