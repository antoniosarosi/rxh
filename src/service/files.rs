//! Static files server sub-service.

use std::path::Path;

use hyper::header;

use crate::http::response::{BoxBodyResponse, LocalResponse};

/// Returns an HTTP response whose body is the content of a file. The file
/// must be located inside the root directory specified by the configuration
/// and must be readable, otherwise a 404 response is returned. This function
/// also assumes that `path` is relative, so it can't start with "/".
pub(super) async fn transfer(path: &str, root: &str) -> Result<BoxBodyResponse, hyper::Error> {
    let Ok(directory) = Path::new(root).canonicalize() else {
        return Ok(LocalResponse::not_found());
    };

    let Ok(file) = directory.join(path).canonicalize() else {
        return Ok(LocalResponse::not_found());
    };

    if !file.starts_with(directory) || !file.is_file() {
        return Ok(LocalResponse::not_found());
    }

    let content_type = match file.extension().and_then(|e| e.to_str()).unwrap_or("txt") {
        "html" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        _ => "text/plain",
    };

    // TODO: gzip medium files, stream large files and set
    // Transfer-Encoding: chunked.
    match tokio::fs::read(file).await {
        Ok(content) => Ok(LocalResponse::builder()
            .header(header::CONTENT_TYPE, content_type)
            .body(crate::http::body::full(content))
            .unwrap()),

        Err(_) => Ok(LocalResponse::not_found()),
    }
}
