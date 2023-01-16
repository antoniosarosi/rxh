use std::net::SocketAddr;

use http::HeaderMap;
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

    /// This is called when the client sends an HTTP/1.1 `Connection: Upgrade`
    /// request. In order to set up the underlying upgraded IO (basically, let
    /// the TCP socket receive any data, don't handle HTTP) we have to give up
    /// ownership on the request. See [`hyper::upgrade::on`], that's the
    /// function used to upgrade the connection and it requires either an owned
    /// [`hyper::Request`] or [`hyper::Response`]. We can also pass a mutable
    /// reference to that function, but in practice this is not true for servers
    /// because the upgraded IO won't be initialized until the server responds
    /// with HTTP 101, which means we have to call [`hyper::upgrade::on`] inside
    /// a different [`tokio::task`] and respond after. Tokio tasks need to own
    /// their data, so no references are allowed unless they are 'static.
    ///
    /// This wouldn't be an issue if we were a normal server, once we upgrade on
    /// the request we don't need it anymore. But as it turns out we are a
    /// reverse proxy, so not only do we have to upgrade on the request, we also
    /// have to forward the same request to the upstream server, wait for the
    /// response, check if the upstream server has agreed to upgrade, and if so
    /// set up a tunnel to allow TCP traffic from the client to the server. Not
    /// precisely "easy".
    ///
    /// The problem comes at "forward the same request to the upstream server"
    /// part. Remember that if we try to upgrade on the request by calling
    /// [`hyper::upgrade::on`] inside a newly spawned [`tokio::task`] we've
    /// already given up ownership on the [`hyper::Request`]. Now, if we want to
    /// send the same request to the upstream server, which is basically what
    /// "forward" means, we still need to maintain ownership on the incoming
    /// [`hyper::Request`] because [`hyper::client::conn::http1::SendRequest`]
    /// requires an owned [`hyper::Request`] in order to be able to send
    /// anything.
    ///
    /// So, when our [`hyper::service::Service`] is called at
    /// [`crate::proxy::Proxy`], we are given one owned [`hyper::Request`], but
    /// due to how the [`hyper`] API is designed we need 2 owned requests. Of
    /// course, [`hyper::Request`] cannot be cloned because the body part
    /// arrives in frames (might not be complete yet), in case we were not
    /// facing enough restrictions so far.
    ///
    /// This function is the solution to the problem described above. Instead
    /// of trying to make the ownership thing work, we simply create two
    /// separate requests that are almost identical and return them as a tuple.
    ///
    /// The first one has the same data (headers & body) as the incoming
    /// request, but does not include the extensions (see [`http::Extensions`]).
    /// Skipping all the details, the extensions allow us to call
    /// [`hyper::upgrade::on`] on a [`hyper::Request`], so they are necessary
    /// if we want to upgrade. Why are we not including extensions in this newly
    /// created request then? Because this is the request that we'll send to the
    /// upstream server, which only needs headers and body. The headers are
    /// clonable, but the body is not, so we're giving up ownership on the body
    /// here.
    ///
    /// On the other hand, the second request that we create has the same
    /// headers as the incoming request and the same extensions (not clonable,
    /// just give up ownership). This is the request that we'll pass as an
    /// argument to [`hyper::upgrade::on`], and the upgrade will work because
    /// [`hyper`] will see that there's an `Upgrade` HTTP header and the
    /// extensions include [`hyper::upgrade::OnUpgrade`], which the library will
    /// use internally to keep the connection alive allowing TCP traffic. The
    /// body is not necessary here, that's why we can give up ownership on the
    /// body in order to create the forwarded request. Similarly, the extensions
    /// are not necessary for the forwarded request, se we can maintain
    /// ownership on them until we create the upgraded request.
    ///
    /// This little hack allows us to send to the upstream server a request that
    /// is identical to the incoming request, including the body (although
    /// upgrade requests usually don't include any body, but just in case) while
    /// still allowing us to obtain an upgraded connection from the incoming
    /// request. Maybe there's a better way of handling this, but we would
    /// probably need to use some private [`hyper`] internals not available on
    /// the public API.
    pub fn into_upgraded(self) -> (ProxyRequest<T>, Request<()>) {
        // Disassemble the incoming request.
        let (parts, body) = self.request.into_parts();

        // Build the "forwarded request", the one we'll send to the backend.
        let mut builder = Request::builder()
            .method(&parts.method)
            .uri(&parts.uri)
            .version(parts.version.clone());
        *builder.headers_mut().unwrap() = parts.headers.clone();

        // This request includes the body from the incoming request.
        let forward_request = Self::new(
            builder.body(body).unwrap(),
            self.client_addr,
            self.server_addr,
        );

        // Build the "upgraded request", which we'll use to proxy upgraded data.
        let mut builder = Request::builder()
            .method(parts.method)
            .uri(parts.uri)
            .version(parts.version.clone());
        *builder.headers_mut().unwrap() = parts.headers;
        *builder.extensions_mut().unwrap() = parts.extensions;

        // This request has no body but includes the extensions from the
        // incoming request.
        let upgrade_request = builder.body(()).unwrap();

        (forward_request, upgrade_request)
    }
}
