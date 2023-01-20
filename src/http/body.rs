//! Utilities for creating common request and response bodies.

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};

/// Single chunk body.
pub fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

#[allow(dead_code)]
pub fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}
