//! Content-hashed static assets (WP14).
//!
//! At startup, [`init`] walks `static_root` and hashes every file into a
//! logical-path → hash manifest (`AssetManifest`). [`PageLayout::asset`]
//! (crate::PageLayout) resolves a logical path (e.g. `"css/app.css"`) to its
//! `/static/h/<hash>/<path>` URL through [`resolve`]; [`hashed_static`] is the
//! handler that serves a file under that URL with a long, immutable
//! `Cache-Control` once the hash in the URL is checked against the manifest.
//! A stale or forged hash 404s rather than serving the file — so a
//! redeployed asset can never keep being served under an old cache key.
//!
//! The manifest is cached process-wide in a `OnceLock` (populated once, at
//! router-build time — production builds the router exactly once at process
//! startup; the test suite's `test_config()` always points `static_root` at
//! the same `web/static` directory, so re-building the router across tests
//! keeps reading a manifest that is still correct). It is also copied onto
//! [`crate::http::AppState`] so the `/static/h/{hash}/{*path}` handler can
//! read it as ordinary request state rather than reaching for the global.

use axum::extract::{Path, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path as FsPath;
use std::sync::{Arc, OnceLock};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::http::AppState;

/// Logical path (forward-slash, relative to `static_root` — e.g.
/// `"css/app.css"`) → the first 10 hex chars of that file's SHA-256 at
/// startup.
pub type AssetManifest = Arc<HashMap<String, String>>;

static MANIFEST: OnceLock<AssetManifest> = OnceLock::new();

/// Builds (or returns the already-built) manifest for `static_root`. Call
/// once, at router-build time.
pub fn init(static_root: &FsPath) -> AssetManifest {
    MANIFEST.get_or_init(|| Arc::new(scan(static_root))).clone()
}

fn scan(root: &FsPath) -> HashMap<String, String> {
    let mut out = HashMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &FsPath, dir: &FsPath, out: &mut HashMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        // A missing/unreadable static_root just means every `asset()` call
        // falls back to the plain, unhashed `/static/...` URL.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(root, &path, out);
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let logical = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hasher.finalize();
        let hash: String = digest.iter().take(5).map(|b| format!("{b:02x}")).collect();
        out.insert(logical, hash);
    }
}

/// Resolves a logical path to its content-hashed URL, or the plain
/// `/static/<path>` when the manifest hasn't been built yet or the path
/// isn't in it (e.g. a file added after startup) — always a working URL,
/// just without the long-lived cache header.
pub fn resolve(logical: &str) -> String {
    match MANIFEST.get().and_then(|m| m.get(logical)) {
        Some(hash) => format!("/static/h/{hash}/{logical}"),
        None => format!("/static/{logical}"),
    }
}

/// GET `/static/h/{hash}/{*path}` — serves `static_root/{path}` with
/// `Cache-Control: public, max-age=31536000, immutable`, but only when
/// `hash` matches the manifest's current hash for that logical path.
pub async fn hashed_static(
    State(state): State<AppState>,
    Path((hash, path)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some(expected) = state.assets.get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if expected != &hash {
        return StatusCode::NOT_FOUND.into_response();
    }
    let file = ServeFile::new(state.config.static_root.join(&path));
    // `ServeFile`'s `Service::Error` is `Infallible` — the `match` below just
    // unwraps it without a panicking `.unwrap()` in the happy path.
    match file.oneshot(req).await {
        Ok(resp) => {
            let (mut parts, body) = resp.into_parts();
            if parts.status == StatusCode::OK {
                parts.headers.insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=31536000, immutable"),
                );
            }
            Response::from_parts(parts, axum::body::Body::new(body))
        }
        Err(infallible) => match infallible {},
    }
}
