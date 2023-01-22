//! Proxy requests require additional information attached to them such as
//! the IP address of the client, which we can't obtain using [`hyper::Request`]
//! alone. We also need to implement different RFCs for request forwarding.

use std::net::SocketAddr;

use http::{Extensions, HeaderMap};
use hyper::{
    header::{self, HeaderValue},
    Request,
};

/// Request received by this proxy from a client.
pub(crate) struct ProxyRequest<T> {
    /// Original client request.
    request: Request<T>,

    /// Client socket.
    client_addr: SocketAddr,

    /// Local socket currently handling this request.
    server_addr: SocketAddr,
}

impl<T> ProxyRequest<T> {
    /// Creates a new [`ProxyRequest`].
    pub fn new(request: Request<T>, client_addr: SocketAddr, server_addr: SocketAddr) -> Self {
        Self {
            request,
            client_addr,
            server_addr,
        }
    }

    pub fn headers(&self) -> &HeaderMap {
        self.request.headers()
    }

    pub fn extensions_mut(&mut self) -> &mut Extensions {
        self.request.extensions_mut()
    }

    /// Consumes the [`ProxyRequest`] returning a [`hyper::Request`] that
    /// contains a valid HTTP forwarded header. This is an implementation of
    /// RFC 7239, see the details in the section below.
    ///
    /// # RFC 7239
    ///
    /// Document: https://www.rfc-editor.org/rfc/rfc7239
    ///
    /// ## Summary
    ///
    /// The HTTP `Forwarded` header allows proxies to disclose information lost
    /// in the proxying process, such as the IP of the original client. This
    /// header is optional.
    ///
    /// ### HTTP `Forwarded` header format:
    ///
    /// ```text
    /// Forwarded: for={};by={};host={};proto={}
    /// ```
    ///
    /// ### HTTP `Forwarded` header parameters:
    ///
    /// - `for`: Identifies the client that initiated the request. This may be
    /// an obfuscated ID or IP + optional TCP port.
    ///
    /// - `by`: Identifies the interface where the request came in to the
    /// proxy server. This may be an obfuscated ID, IP or IP and TCP port.
    ///
    /// - `host`: Original value of the `Host` HTTP header, as received by
    /// the proxy.
    ///
    /// - `proto`: Protocol used to make the request. For example, `http` or
    /// `https`.
    ///
    /// All parameters are optional. IP addresses may be IPv4 or IPv6. If the
    /// address is IPv6 it must be enclosed in square brackets.
    ///
    /// ### HTTP Lists
    ///
    /// If the request passes through multiple proxies, this header could
    /// contain a list of values separated by commas.
    ///
    /// ### Example
    ///
    /// A request from a client with IP address 192.0.2.43 passes through a
    /// proxy with IP address 198.51.100.17, then through another proxy with IP
    /// address 203.0.113.60 before reaching an origin server.
    ///
    /// The HTTP request between the client and the first proxy has no
    /// `Forwarded` header field.
    ///
    /// The HTTP request between the first and second proxy has a
    /// `Forwarded: for=192.0.2.43` header field.
    ///
    /// The HTTP request between the second proxy and the origin server
    /// contains:
    ///
    /// ```text
    /// Forwarded: for=192.0.2.43, for=198.51.100.17;by=203.0.113.60;proto=http;host=example.com
    /// ```
    ///
    /// ### Security Considerations
    ///
    /// There is nothing that can be trusted in this header, as every proxy
    /// in the chain can manipulate the value. Even the original client can
    /// set any value to the `Forwarded` header.
    pub fn into_forwarded(mut self) -> Request<T> {
        let host = if let Some(value) = self.request.headers().get(header::HOST) {
            match value.to_str() {
                Ok(host) => String::from(host),
                Err(_) => self.server_addr.to_string(),
            }
        } else {
            self.server_addr.to_string()
        };

        // TODO: Proto
        let mut forwarded = format!(
            "for={};by={};host={}",
            self.client_addr, self.server_addr, host
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_request() {
        let client = "127.0.0.1:8000".parse().unwrap();
        let proxy = "127.0.0.1:9000".parse().unwrap();

        let request = ProxyRequest::new(
            Request::builder().body(crate::http::body::empty()).unwrap(),
            client,
            proxy,
        );

        let forwarded = request.into_forwarded();
        let expected = format!("for={client};by={proxy};host={proxy}");

        assert!(forwarded.headers().contains_key(header::FORWARDED));
        assert_eq!(
            forwarded
                .headers()
                .get(header::FORWARDED)
                .unwrap()
                .to_str()
                .unwrap(),
            expected.as_str()
        );
    }
}
