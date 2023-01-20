use hyper::header;

use crate::http::response::{BoxBodyResponse, LocalResponse};

/// Returns an HTTP response whose body is the content of a file. The file
/// must be located inside the root directory specified by the configuration
/// and must be readable, otherwise a 404 response is returned.
pub(super) async fn transfer(path: &str, root: &str) -> Result<BoxBodyResponse, hyper::Error> {
    let path = std::path::Path::new(root).join(path);
    println!("Send file {}", path.to_str().unwrap());

    if !path
        .canonicalize()
        .is_ok_and(|path| path.starts_with(root) && path.is_file())
    {
        return Ok(LocalResponse::not_found());
    }

    let mut content_type = "text/plain";

    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        content_type = match extension {
            "html" => "text/html",
            "css" => "text/css",
            "js" => "application/javascript",
            "png" => "image/png",
            "jpeg" => "image/jpeg",
            _ => "text/plain",
        }
    }

    // TODO: gzip medium files, stream large files and set
    // Transfer-Encoding: chunked.
    match tokio::fs::read(path).await {
        Ok(content) => Ok(LocalResponse::builder()
            .header(header::CONTENT_TYPE, content_type)
            .body(crate::http::body::full(content))
            .unwrap()),

        Err(_) => Ok(LocalResponse::not_found()),
    }
}
