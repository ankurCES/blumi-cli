//! Embedded frontend assets.
//!
//! In release builds the `frontend/dist` directory is baked into the binary; in
//! debug builds rust-embed reads it from disk (so `npm run build` + refresh is a
//! fast loop). Unknown paths fall back to `index.html` so the SPA router works.

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::Embed)]
#[folder = "frontend/dist"]
struct Assets;

/// Serve an embedded asset, falling back to `index.html` for SPA routes.
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.into_owned(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html for any unknown non-API path.
    match Assets::get("index.html") {
        Some(content) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content.data.into_owned(),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Body::from("frontend not built — run `npm run build` in crates/blumi-web/frontend"),
        )
            .into_response(),
    }
}
