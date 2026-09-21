//! The built frontend (web/dist), compiled into the binary so an installed
//! `ridge_redux` needs nothing beside it. build.rs picks the directory.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$RIDGE_FRONTEND_DIR"]
struct Assets;

/// Serve an embedded file, with `/` as `index.html`. Astro's hashed assets
/// under `_astro/` never change, so browsers may keep them.
pub async fn serve(uri: Uri) -> Response {
    let path = match uri.path().trim_start_matches('/') {
        "" => "index.html",
        p => p,
    };
    let Some(file) = Assets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let cache = if path.starts_with("_astro/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        file.data,
    )
        .into_response()
}
