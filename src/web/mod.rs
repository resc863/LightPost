pub mod admin;
pub mod assets;
pub mod auth;
pub mod feed;
pub mod public;

use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};

use crate::{
    config::Config,
    db::Repository,
    error::{AppError, ErrorPage},
    render,
};

/// Largest page body the ETag layer will buffer.
const MAX_ETAG_BODY: usize = 8 * 1024 * 1024;

const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; img-src 'self' https: data:; style-src 'self'; \
    script-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'";

#[derive(Clone)]
pub struct AppState {
    pub repository: Arc<Repository>,
    pub config: Arc<Config>,
    sessions: Arc<auth::Sessions>,
}

impl AppState {
    pub fn new(repository: Repository, config: Config) -> Self {
        Self {
            repository: Arc::new(repository),
            config: Arc::new(config),
            sessions: Arc::new(auth::Sessions::default()),
        }
    }

    /// An empty in-memory blog with default settings.
    pub fn in_memory(admin_token: impl Into<String>) -> Self {
        Self::with_config(Config::new(admin_token))
    }

    pub fn with_config(config: Config) -> Self {
        let repository = Repository::in_memory().expect("in-memory database should open");
        Self::new(repository, config)
    }

    /// Runs blocking repository work on Tokio's blocking pool.
    pub async fn db<T, F>(&self, work: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&Repository) -> Result<T, AppError> + Send + 'static,
    {
        let repository = Arc::clone(&self.repository);
        tokio::task::spawn_blocking(move || work(&repository)).await?
    }
}

pub fn build_app(state: AppState) -> Router {
    let public = Router::new()
        .route("/", get(public::home))
        .route("/posts/{slug}", get(public::post_page))
        .route("/search", get(public::search))
        .route("/tags/{tag}", get(public::tag_page))
        .route("/feed.xml", get(feed::rss))
        .route("/sitemap.xml", get(feed::sitemap))
        .fallback(public::not_found)
        .layer(middleware::from_fn(etag));

    let admin_pages = Router::new()
        .route("/admin", get(admin::dashboard))
        .route("/admin/login", get(auth::login_page).post(auth::login))
        .route("/admin/logout", post(auth::logout))
        .route("/admin/posts", post(admin::create_post))
        .route("/admin/posts/new", get(admin::new_post))
        .route("/admin/posts/{id}", post(admin::update_post))
        .route("/admin/posts/{id}/edit", get(admin::edit_post))
        .route("/admin/posts/{id}/delete", post(admin::delete_post))
        .layer(middleware::map_response(no_store));

    let pages = public
        .merge(admin_pages)
        .layer(middleware::from_fn_with_state(state.clone(), error_pages));

    let api = Router::new()
        .route(
            "/api/admin/posts",
            get(admin::api_list).post(admin::api_create),
        )
        .route(
            "/api/admin/posts/{id}",
            get(admin::api_get)
                .put(admin::api_update)
                .delete(admin::api_delete),
        )
        .layer(middleware::map_response(no_store));

    Router::new()
        .merge(pages)
        .merge(api)
        .route("/static/{file}", get(assets::serve))
        .route("/healthz", get(health))
        .layer(middleware::map_response(security_headers))
        .layer(middleware::from_fn(access_log))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

/// Replaces the plain-text body of an `AppError` with a page in the site layout.
async fn error_pages(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    match response.extensions().get::<ErrorPage>() {
        Some(page) => (
            page.status,
            Html(render::error_page(
                &state.config,
                page.status.as_u16(),
                &page.message,
            )),
        )
            .into_response(),
        None => response,
    }
}

/// Adds a content hash ETag to successful GET responses and answers
/// `If-None-Match` with 304, so unchanged pages and feeds aren't resent.
async fn etag(request: Request, next: Next) -> Response {
    let is_get = matches!(*request.method(), Method::GET | Method::HEAD);
    let if_none_match = request.headers().get(header::IF_NONE_MATCH).cloned();
    let response = next.run(request).await;
    if !is_get || response.status() != StatusCode::OK {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_ETAG_BODY).await else {
        return AppError::Internal("response body too large for ETag".to_owned()).into_response();
    };
    let tag = format!("\"{:016x}\"", assets::fnv1a64(&bytes));
    let value = HeaderValue::from_str(&tag).expect("hex ETag is a valid header value");
    parts.headers.insert(header::ETAG, value.clone());
    parts
        .headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-cache"));
    if if_none_match.is_some_and(|candidates| etag_matches(&candidates, &tag)) {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, Body::from(bytes))
}

fn etag_matches(header: &HeaderValue, tag: &str) -> bool {
    header.to_str().is_ok_and(|value| {
        value
            .split(',')
            .map(|candidate| candidate.trim().trim_start_matches("W/"))
            .any(|candidate| candidate == tag || candidate == "*")
    })
}

async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn security_headers(mut response: Response) -> Response {
    let headers: &mut HeaderMap = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "strict-origin-when-cross-origin"),
    ] {
        headers
            .entry(name)
            .or_insert(HeaderValue::from_static(value));
    }
    response
}

/// One line per request on stderr: method, path, status, duration.
async fn access_log(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    eprintln!(
        "{method} {path} {} {}ms",
        response.status().as_u16(),
        started.elapsed().as_millis()
    );
    response
}
