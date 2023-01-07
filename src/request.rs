use std::net::SocketAddr;

use hyper::{
    header::{self, HeaderValue},
    Request,
};

pub(crate) struct RxhRequest<T> {
    request: Request<T>,
    client_addr: SocketAddr,
    server_addr: SocketAddr,
}

impl<T> RxhRequest<T> {
    /// Consumes the [`RxhRequest`] returning a [`hyper::Request`] that contains
    /// a valid HTTP forwarded header.
    pub fn into_forwarded(mut self) -> Request<T> {
        let host = if let Some(value) = self.request.headers().get(header::HOST) {
            match value.to_str() {
                Ok(host) => String::from(host),
                Err(_) => self.server_addr.to_string(),
            }
        } else {
            self.server_addr.to_string()
        };

        let mut forwarded = format!(
            "by={};for={};host={}",
            self.server_addr, self.client_addr, host
        );

        if let Some(value) = self.request.headers().get(header::FORWARDED) {
            if let Ok(previous_proxies) = value.to_str() {
                forwarded = format!("{previous_proxies}, {forwarded}");
            }
        }

        self.request.headers_mut().insert(
            header::FORWARDED,
            HeaderValue::from_str(&forwarded).unwrap(),
        );

        self.request
    }
}
