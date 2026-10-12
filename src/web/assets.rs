//! Static files compiled into the binary, so the server runs from any directory.

use std::sync::LazyLock;

use axum::{
    extract::{Path, RawQuery},
    http::header,
    response::{IntoResponse, Response},
};

use crate::error::AppError;

struct Asset {
    name: &'static str,
    content_type: &'static str,
    body: &'static str,
}

const ASSETS: &[Asset] = &[
    Asset {
        name: "style.css",
        content_type: "text/css; charset=utf-8",
        body: include_str!("../../static/style.css"),
    },
    Asset {
        name: "app.js",
        content_type: "text/javascript; charset=utf-8",
        body: include_str!("../../static/app.js"),
    },
    Asset {
        name: "favicon.svg",
        content_type: "image/svg+xml",
        body: include_str!("../../static/favicon.svg"),
    },
];

static STYLE_URL: LazyLock<String> = LazyLock::new(|| versioned_url("style.css"));
static SCRIPT_URL: LazyLock<String> = LazyLock::new(|| versioned_url("app.js"));
static FAVICON_URL: LazyLock<String> = LazyLock::new(|| versioned_url("favicon.svg"));

/// `/static/style.css?v=<content hash>`; the hash changes whenever the file does.
pub fn style_url() -> &'static str {
    &STYLE_URL
}

pub fn script_url() -> &'static str {
    &SCRIPT_URL
}

pub fn favicon_url() -> &'static str {
    &FAVICON_URL
}

fn versioned_url(name: &str) -> String {
    let asset = find(name).expect("asset is embedded");
    format!("/static/{name}?v={:016x}", fnv1a64(asset.body.as_bytes()))
}

fn find(name: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.name == name)
}

pub async fn serve(
    Path(file): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let asset = find(&file).ok_or(AppError::NotFound)?;
    // Versioned URLs never change content, so browsers may keep them for a year.
    let cache_control = if query.is_some_and(|query| query.starts_with("v=")) {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=300"
    };
    Ok((
        [
            (header::CONTENT_TYPE, asset.content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        asset.body,
    )
        .into_response())
}

/// 64-bit FNV-1a: a fast, stable fingerprint for cache keys (not for security).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
