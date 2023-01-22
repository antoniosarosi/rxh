//! The [`hyper`] library is based on services. Each time we accept a connection
//! we have to provide an instance of [`hyper::service::Service`] to handle that
//! connection. This module contains the [`Rxh`] struct which implements
//! [`hyper::service::Service`] and handles requests based on the configuration
//! file. The particular configuration for an instance of [`Rxh`] is provided
//! by a [`crate::server::Server`], and might contain multiple actions such as
//! "serve files from this directory if the URI starts with /website" or
//! "forward the request to an upstream server otherwise".

mod files;
mod proxy;

use std::{future::Future, net::SocketAddr, pin::Pin};

use hyper::{body::Incoming, service::Service, Request};

use crate::{
    config::{self, Action, Forward},
    http::{
        request::ProxyRequest,
        response::{BoxBodyResponse, LocalResponse},
    },
};

/// Implements [`Service`] and handles incoming requests.
pub(crate) struct Rxh {
    /// Reference to the configuration of this [`crate::server::Server`]
    /// instance.
    config: &'static config::Server,

    // Socket address of the connected client.
    client_addr: SocketAddr,

    // Listening socket address.
    server_addr: SocketAddr,
}

impl Rxh {
    /// Creates a new [`Rxh`] service.
    pub fn new(
        config: &'static config::Server,
        client_addr: SocketAddr,
        server_addr: SocketAddr,
    ) -> Self {
        Self {
            config,
            client_addr,
            server_addr,
        }
    }
}

impl Service<Request<Incoming>> for Rxh {
    type Response = BoxBodyResponse;

    type Error = hyper::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, request: Request<Incoming>) -> Self::Future {
        let Rxh {
            client_addr,
            server_addr,
            config,
        } = *self;

        Box::pin(async move {
            let uri = request.uri().to_string();
            let method = request.method().to_string();

            let maybe_pattern = config
                .patterns
                .iter()
                .find(|pattern| uri.starts_with(pattern.uri.as_str()));

            let Some(pattern) = maybe_pattern else {
                return Ok(LocalResponse::not_found());
            };

            let response = match &pattern.action {
                Action::Forward(Forward { scheduler, .. }) => {
                    let by = config.name.as_ref().map(|name| name.clone());
                    let request = ProxyRequest::new(request, client_addr, server_addr, by);
                    proxy::forward(request, scheduler.next_server()).await
                }

                Action::Serve(directory) => {
                    let path = if request.uri().path().starts_with("/") {
                        &request.uri().path()[1..]
                    } else {
                        request.uri().path()
                    };
                    files::transfer(path, directory).await
                }
            };

            if let Ok(response) = &response {
                let status = response.status();
                println!("{client_addr} -> {server_addr} {method} {uri} HTTP {status}");
            }

            response
        })
    }
}
