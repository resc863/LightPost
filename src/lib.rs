use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use pulldown_cmark::{html, Parser};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;

const SESSION_COOKIE: &str = "lightpost_session";

#[derive(Clone)]
pub struct AppState {
    pub repository: Arc<BlogRepository>,
    pub admin_token: Arc<String>,
}

impl AppState {
    pub fn new(repository: Arc<BlogRepository>, admin_token: impl Into<String>) -> Self {
        Self {
            repository,
            admin_token: Arc::new(admin_token.into()),
        }
    }

    pub fn in_memory(admin_token: impl Into<String>) -> Self {
        let repository = BlogRepository::in_memory().expect("in-memory database should open");
        Self::new(Arc::new(repository), admin_token)
    }
}

pub struct BlogRepository {
    connection: Mutex<Connection>,
}

impl BlogRepository {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        let repository = Self {
            connection: Mutex::new(connection),
        };
        repository.initialize()?;
        Ok(repository)
    }

    pub fn in_memory() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        let repository = Self {
            connection: Mutex::new(connection),
        };
        repository.initialize()?;
        Ok(repository)
    }

    fn initialize(&self) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("repository mutex poisoned");
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS posts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                excerpt TEXT NOT NULL DEFAULT '',
                body_markdown TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL CHECK(status IN ('draft', 'published')),
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                published_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_posts_status_created ON posts(status, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_posts_slug ON posts(slug);",
        )?;
        Ok(())
    }

    pub fn create_post(&self, input: &NewPost) -> rusqlite::Result<Post> {
        let status = normalize_status(&input.status);
        let slug = if input.slug.trim().is_empty() {
            slugify(&input.title)
        } else {
            slugify(&input.slug)
        };
        if slug.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "title must produce a slug".to_owned(),
            ));
        }
        let connection = self.connection.lock().expect("repository mutex poisoned");
        connection.execute(
            "INSERT INTO posts (slug, title, excerpt, body_markdown, tags, status, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?6 = 'published' THEN CURRENT_TIMESTAMP ELSE NULL END)",
            params![slug, input.title.trim(), input.excerpt.trim(), input.body_markdown, normalize_tags(&input.tags), status],
        )?;
        let id = connection.last_insert_rowid();
        drop(connection);
        self.get_by_id(id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn get_by_id(&self, id: i64) -> rusqlite::Result<Option<Post>> {
        let connection = self.connection.lock().expect("repository mutex poisoned");
        connection.query_row("SELECT id, slug, title, excerpt, body_markdown, tags, status, created_at, updated_at, published_at FROM posts WHERE id = ?1", [id], post_from_row).optional()
    }

    pub fn get_by_slug(&self, slug: &str) -> rusqlite::Result<Option<Post>> {
        let connection = self.connection.lock().expect("repository mutex poisoned");
        connection.query_row("SELECT id, slug, title, excerpt, body_markdown, tags, status, created_at, updated_at, published_at FROM posts WHERE slug = ?1", [slug], post_from_row).optional()
    }

    pub fn list_published(&self) -> rusqlite::Result<Vec<Post>> {
        let connection = self.connection.lock().expect("repository mutex poisoned");
        let mut statement = connection.prepare("SELECT id, slug, title, excerpt, body_markdown, tags, status, created_at, updated_at, published_at FROM posts WHERE status = 'published' ORDER BY created_at DESC")?;
        let posts = statement
            .query_map([], post_from_row)?
            .collect::<Result<Vec<_>, _>>();
        posts
    }

    pub fn search_published(&self, term: &str) -> rusqlite::Result<Vec<Post>> {
        let pattern = format!("%{}%", term.trim());
        let connection = self.connection.lock().expect("repository mutex poisoned");
        let mut statement = connection.prepare("SELECT id, slug, title, excerpt, body_markdown, tags, status, created_at, updated_at, published_at FROM posts WHERE status = 'published' AND (title LIKE ?1 OR excerpt LIKE ?1 OR body_markdown LIKE ?1 OR tags LIKE ?1) ORDER BY created_at DESC")?;
        let posts = statement
            .query_map([pattern], post_from_row)?
            .collect::<Result<Vec<_>, _>>();
        posts
    }

    pub fn list_by_tag(&self, tag: &str) -> rusqlite::Result<Vec<Post>> {
        let pattern = format!("%,{},%", tag.trim().to_lowercase());
        let connection = self.connection.lock().expect("repository mutex poisoned");
        let mut statement = connection.prepare("SELECT id, slug, title, excerpt, body_markdown, tags, status, created_at, updated_at, published_at FROM posts WHERE status = 'published' AND (',' || tags || ',') LIKE ?1 ORDER BY created_at DESC")?;
        let posts = statement
            .query_map([pattern], post_from_row)?
            .collect::<Result<Vec<_>, _>>();
        posts
    }
}

fn post_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Post> {
    Ok(Post {
        id: row.get(0)?,
        slug: row.get(1)?,
        title: row.get(2)?,
        excerpt: row.get(3)?,
        body_markdown: row.get(4)?,
        tags: split_tags(&row.get::<_, String>(5)?),
        status: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        published_at: row.get(9)?,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub excerpt: String,
    pub body_markdown: String,
    pub tags: Vec<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewPost {
    pub title: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub excerpt: String,
    pub body_markdown: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "draft".to_owned()
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
}

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/posts/{slug}", get(post_page))
        .route("/search", get(search_page))
        .route("/tags/{tag}", get(tag_page))
        .route("/feed.xml", get(feed))
        .route("/sitemap.xml", get(sitemap))
        .route("/healthz", get(health))
        .route("/admin", get(admin_page))
        .route("/api/admin/posts", post(create_post))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn home(State(state): State<AppState>) -> Response {
    match state.repository.list_published() {
        Ok(posts) => Html(render_layout("기록", &format!("<section class=\"intro\"><p class=\"eyebrow\">A PERSONAL JOURNAL</p><h1>작은 생각을 모아,<br>오래 남을 기록으로.</h1><p class=\"intro-description\">일상에서 발견한 것들, 배우며 쌓아가는 생각들.<br>천천히 쓰고 오래 읽는 공간입니다.</p></section><section aria-labelledby=\"posts-heading\"><div class=\"section-heading\"><h2 id=\"posts-heading\">최근 기록</h2><span>{}개의 글</span></div>{}</section>", posts.len(), render_post_list(&posts)))).into_response(),
        Err(error) => internal_error(error),
    }
}

async fn post_page(State(state): State<AppState>, AxumPath(slug): AxumPath<String>) -> Response {
    match state.repository.get_by_slug(&slug) {
        Ok(Some(post)) if post.status == "published" => {
            Html(render_layout(&post.title, &render_post(&post))).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => internal_error(error),
    }
}

async fn search_page(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Response {
    let term = params.q.unwrap_or_default();
    match state.repository.search_published(&term) {
        Ok(posts) => Html(render_layout("검색", &format!("<section class=\"page-intro\"><p class=\"eyebrow\">EXPLORE THE JOURNAL</p><h1>기록을 찾아서</h1><p class=\"intro-description\">궁금한 단어나 주제로 지난 생각들을 찾아보세요.</p></section><form class=\"search\" action=\"/search\" role=\"search\"><label class=\"sr-only\" for=\"search-query\">검색어</label><input id=\"search-query\" type=\"search\" name=\"q\" value=\"{}\" placeholder=\"어떤 기록을 찾고 있나요?\"><button type=\"submit\">검색 <span aria-hidden=\"true\">↗</span></button></form><div class=\"section-heading\"><h2>검색 결과</h2><span>{}개의 글</span></div>{}", escape_html(&term), posts.len(), render_post_list(&posts)))).into_response(),
        Err(error) => internal_error(error),
    }
}

async fn tag_page(State(state): State<AppState>, AxumPath(tag): AxumPath<String>) -> Response {
    match state.repository.list_by_tag(&tag) {
        Ok(posts) => Html(render_layout(
            &format!("태그: {tag}"),
            &format!(
                "<section class=\"page-intro\"><p class=\"eyebrow\">COLLECTED NOTES</p><h1>#{}</h1><p class=\"intro-description\">하나의 주제로 이어지는 기록들.</p></section>{}",
                escape_html(&tag),
                render_post_list(&posts)
            ),
        ))
        .into_response(),
        Err(error) => internal_error(error),
    }
}

async fn feed(State(state): State<AppState>) -> Response {
    match state.repository.list_published() {
        Ok(posts) => (
            [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
            render_rss(&posts),
        )
            .into_response(),
        Err(error) => internal_error(error),
    }
}

async fn sitemap(State(state): State<AppState>) -> Response {
    match state.repository.list_published() {
        Ok(posts) => (
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            render_sitemap(&posts),
        )
            .into_response(),
        Err(error) => internal_error(error),
    }
}

async fn admin_page(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_admin(&headers, &state.admin_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Html(render_layout("관리자", "<section class=\"page-intro\"><p class=\"eyebrow\">WRITE A NOTE</p><h1>새로운 기록</h1><p class=\"intro-description\">오늘의 생각을 천천히 적어보세요.</p></section><form id=\"post-form\" class=\"editor\"><label>제목<input name=\"title\" placeholder=\"기록에 이름을 붙여주세요\" required></label><label>본문 <span class=\"field-hint\">Markdown으로 작성할 수 있어요</span><textarea name=\"body_markdown\" placeholder=\"이곳에서 이야기를 시작하세요.\" required></textarea></label><div class=\"editor-actions\"><span class=\"meta\">저장한 글은 초안으로 보관됩니다.</span><button>초안 저장 ↗</button></div></form><script type=module src=/static/app.js></script>")).into_response()
}

async fn create_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(input): Json<NewPost>,
) -> Response {
    if !is_admin(&headers, &state.admin_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.title.trim().is_empty() || input.body_markdown.trim().is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match state.repository.create_post(&input) {
        Ok(post) => (StatusCode::CREATED, Json(post)).into_response(),
        Err(rusqlite::Error::SqliteFailure(_, _)) => StatusCode::CONFLICT.into_response(),
        Err(error) => internal_error(error),
    }
}

fn is_admin(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("{SESSION_COOKIE}={token}");
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .map(|cookies| cookies.split(';').any(|cookie| cookie.trim() == expected))
        .unwrap_or(false)
}

fn internal_error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error: {error}"),
    )
        .into_response()
}

pub fn render_markdown(markdown: &str) -> String {
    let parser = Parser::new(markdown);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

pub fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for character in value.trim().to_lowercase().chars() {
        if character.is_alphanumeric() {
            slug.push(character);
        } else if (character == ' ' || character == '-' || character == '_') && !slug.ends_with('-')
        {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn normalize_status(value: &str) -> &'static str {
    if value.eq_ignore_ascii_case("published") {
        "published"
    } else {
        "draft"
    }
}
fn normalize_tags(tags: &[String]) -> String {
    tags.iter()
        .map(|tag| tag.trim().to_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}
fn split_tags(tags: &str) -> Vec<String> {
    tags.split(',')
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_layout(title: &str, content: &str) -> String {
    format!("<!doctype html><html lang=\"ko\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"theme-color\" content=\"#faf9f6\"><title>{} · LightPost</title><link rel=\"stylesheet\" href=\"/static/style.css\"><link rel=\"alternate\" type=\"application/rss+xml\" title=\"LightPost RSS\" href=\"/feed.xml\"></head><body><a class=\"skip-link\" href=\"#main\">본문으로 건너뛰기</a><div class=\"site-shell\"><header class=\"site-header\"><a class=\"wordmark\" href=\"/\">LightPost<span aria-hidden=\"true\">.</span></a><nav aria-label=\"주 메뉴\"><a href=\"/\">기록</a><a href=\"/search\">검색</a><a class=\"rss-link\" href=\"/feed.xml\">RSS <span aria-hidden=\"true\">↗</span></a></nav></header><main id=\"main\">{content}</main><footer class=\"site-footer\"><div><a class=\"footer-brand\" href=\"/\">LightPost.</a><p>작은 기록이 쌓이는 곳.</p></div><a href=\"/admin\">글 쓰기 <span aria-hidden=\"true\">↗</span></a></footer></div></body></html>", escape_html(title))
}
fn render_post_list(posts: &[Post]) -> String {
    if posts.is_empty() {
        return "<div class=\"empty-state\"><span class=\"empty-mark\" aria-hidden=\"true\">✳</span><h2>아직 글이 없습니다.</h2><p>새로운 기록이 쌓이면 이곳에서 만나볼 수 있어요.</p><a href=\"/\">전체 기록 보기 <span aria-hidden=\"true\">↗</span></a></div>".to_owned();
    }
    posts.iter().map(|post| format!("<article class=\"post-row\"><p class=\"post-date\">{}</p><div class=\"post-summary\"><h3><a href=\"/posts/{}\">{}</a></h3><p>{}</p><div class=\"tags\">{}</div></div><span class=\"row-arrow\" aria-hidden=\"true\">↗</span></article>", display_date(post), escape_html(&post.slug), escape_html(&post.title), escape_html(&post.excerpt), render_tags(post))).collect::<Vec<_>>().join("\n")
}
fn display_date(post: &Post) -> String {
    escape_html(
        &post
            .published_at
            .as_deref()
            .unwrap_or(&post.created_at)
            .chars()
            .take(10)
            .collect::<String>(),
    )
    .replace('-', ".")
}
fn render_tags(post: &Post) -> String {
    post.tags
        .iter()
        .map(|tag| {
            format!(
                "<a href=\"/tags/{}\">#{}</a>",
                escape_html(tag),
                escape_html(tag)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn render_post(post: &Post) -> String {
    format!("<article class=\"post-detail\"><a class=\"back-link\" href=\"/\">← 전체 기록</a><header class=\"article-heading\"><p class=\"eyebrow\">{}</p><h1>{}</h1><div class=\"tags\">{}</div></header><div class=\"prose\">{}</div><div class=\"article-end\"><span aria-hidden=\"true\">✳</span><a href=\"/\">다른 기록 읽기 ↗</a></div></article>", display_date(post), escape_html(&post.title), render_tags(post), render_markdown(&post.body_markdown))
}
fn render_rss(posts: &[Post]) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><rss version=\"2.0\"><channel><title>LightPost</title>{}</channel></rss>", posts.iter().map(|post| format!("<item><title>{}</title><link>/posts/{}</link><description>{}</description></item>", escape_html(&post.title), escape_html(&post.slug), escape_html(&post.excerpt))).collect::<Vec<_>>().join(""))
}
fn render_sitemap(posts: &[Post]) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{}</urlset>", posts.iter().map(|post| format!("<url><loc>/posts/{}</loc></url>", escape_html(&post.slug))).collect::<Vec<_>>().join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn post(title: &str, status: &str) -> NewPost {
        NewPost {
            title: title.to_owned(),
            slug: String::new(),
            excerpt: "요약".to_owned(),
            body_markdown: "# 본문\n\n내용".to_owned(),
            tags: vec!["Rust".to_owned(), "Axum".to_owned()],
            status: status.to_owned(),
        }
    }

    #[test]
    fn markdown_is_rendered_to_html() {
        let output = render_markdown("# 제목\n\n**강조**");
        assert!(output.contains("<h1>제목</h1>"));
        assert!(output.contains("<strong>강조</strong>"));
    }

    #[test]
    fn slugify_creates_stable_url_segment() {
        assert_eq!(slugify(" Hello, Rust World! "), "hello-rust-world");
    }

    #[test]
    fn repository_hides_drafts_from_public_queries() {
        let repository = BlogRepository::in_memory().unwrap();
        repository
            .create_post(&post("공개 글", "published"))
            .unwrap();
        repository.create_post(&post("비공개 글", "draft")).unwrap();
        let published = repository.list_published().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(
            repository.get_by_slug("비공개-글").unwrap().unwrap().status,
            "draft"
        );
    }

    #[tokio::test]
    async fn public_post_page_returns_published_content() {
        let state = AppState::in_memory("secret");
        let created = state
            .repository
            .create_post(&post("테스트 글", "published"))
            .unwrap();
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/posts/{}", created.slug))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("테스트 글"));
        assert!(body.contains("<h1>본문</h1>"));
    }

    #[tokio::test]
    async fn draft_post_page_is_not_public() {
        let state = AppState::in_memory("secret");
        let created = state
            .repository
            .create_post(&post("초안", "draft"))
            .unwrap();
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/posts/{}", created.slug))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn admin_api_requires_session_cookie() {
        let app = build_app(AppState::in_memory("secret"));
        let body = serde_json::to_vec(&post("관리 글", "published")).unwrap();
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/posts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let authorized = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/posts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, "lightpost_session=secret")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn rss_does_not_include_drafts() {
        let state = AppState::in_memory("secret");
        state
            .repository
            .create_post(&post("공개 RSS", "published"))
            .unwrap();
        state
            .repository
            .create_post(&post("초안 RSS", "draft"))
            .unwrap();
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/feed.xml")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("공개 RSS"));
        assert!(!body.contains("초안 RSS"));
    }

    #[tokio::test]
    async fn search_endpoint_only_returns_published_matches() {
        let state = AppState::in_memory("secret");
        state
            .repository
            .create_post(&post("Rust 공개 검색", "published"))
            .unwrap();
        state
            .repository
            .create_post(&post("Rust 초안 검색", "draft"))
            .unwrap();
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/search?q=Rust")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("Rust 공개 검색"));
        assert!(!body.contains("Rust 초안 검색"));
    }

    #[tokio::test]
    async fn admin_api_rejects_empty_content() {
        let app = build_app(AppState::in_memory("secret"));
        let body = serde_json::to_vec(&NewPost {
            title: String::new(),
            slug: String::new(),
            excerpt: String::new(),
            body_markdown: String::new(),
            tags: Vec::new(),
            status: "draft".to_owned(),
        })
        .unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/posts")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, "lightpost_session=secret")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
