//! End-to-end tests through the router, with an in-memory database.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use lightpost::{build_app, AppState, Config, PostInput, PostStatus};
use tower::ServiceExt;

const TOKEN: &str = "test-admin-token-123";

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: String::from_utf8(body.to_vec()).unwrap(),
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn post(title: &str, status: PostStatus, tags: &[&str]) -> PostInput {
    PostInput {
        title: title.to_owned(),
        excerpt: "요약".to_owned(),
        body_markdown: "# 본문\n\n내용".to_owned(),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        status,
        ..PostInput::default()
    }
}

fn app_with(posts: &[PostInput]) -> (AppState, Router) {
    let mut config = Config::new(TOKEN);
    config.base_url = "https://blog.example".to_owned();
    let state = AppState::with_config(config);
    for input in posts {
        state.repository.create_post(input).unwrap();
    }
    let app = build_app(state.clone());
    (state, app)
}

/// Logs in through the form and returns the `Cookie` header to send afterwards.
async fn login(app: &Router) -> String {
    let reply = send(
        app,
        Request::post("/admin/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("token={TOKEN}")))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    let set_cookie = reply.headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned();
    set_cookie.split(';').next().unwrap().to_owned()
}

fn form_post(uri: &str, cookie: &str, body: &str) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .header(header::HOST, "blog.example")
        .header(header::ORIGIN, "https://blog.example")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

fn api(method: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn public_post_page_returns_published_content() {
    let (_, app) = app_with(&[post("테스트 글", PostStatus::Published, &[])]);
    let reply = send(&app, get("/posts/%ED%85%8C%EC%8A%A4%ED%8A%B8-%EA%B8%80")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("테스트 글"));
    assert!(reply.body.contains("<h1>본문</h1>"));
    assert!(reply
        .body
        .contains("rel=\"canonical\" href=\"https://blog.example/posts/%ED%85%8C"));
}

#[tokio::test]
async fn draft_post_page_is_a_404_page() {
    let (_, app) = app_with(&[post("초안", PostStatus::Draft, &[])]);
    let reply = send(&app, get("/posts/%EC%B4%88%EC%95%88")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.body.contains("<!DOCTYPE html>"));
    assert!(reply.body.contains("찾을 수 없는 페이지"));
}

#[tokio::test]
async fn unknown_routes_render_the_404_page() {
    let (_, app) = app_with(&[]);
    let reply = send(&app, get("/nothing/here")).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.body.contains("site-shell"));
}

#[tokio::test]
async fn admin_pages_redirect_to_login_without_session() {
    let (_, app) = app_with(&[]);
    let reply = send(&app, get("/admin")).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.headers[header::LOCATION], "/admin/login");
    // The raw token is no longer accepted as a cookie value.
    let reply = send(
        &app,
        Request::get("/admin")
            .header(header::COOKIE, format!("lightpost_session={TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn login_sets_a_hardened_session_cookie() {
    let (_, app) = app_with(&[]);
    let wrong = send(
        &app,
        Request::post("/admin/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("token=wrong"))
            .unwrap(),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.headers.get(header::SET_COOKIE).is_none());

    let reply = send(
        &app,
        Request::post("/admin/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("token={TOKEN}")))
            .unwrap(),
    )
    .await;
    let cookie = reply.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("Secure"), "base_url is https");
    assert!(!cookie.contains(TOKEN));

    let cookie = login(&app).await;
    let dashboard = send(
        &app,
        Request::get("/admin")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(dashboard.status, StatusCode::OK);
    assert_eq!(dashboard.headers[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn logout_ends_the_session() {
    let (_, app) = app_with(&[]);
    let cookie = login(&app).await;
    let reply = send(&app, form_post("/admin/logout", &cookie, "")).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    let dashboard = send(
        &app,
        Request::get("/admin")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(dashboard.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn cross_site_form_posts_are_rejected() {
    let (_, app) = app_with(&[]);
    let cookie = login(&app).await;
    let reply = send(
        &app,
        Request::post("/admin/posts")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &cookie)
            .header(header::HOST, "blog.example")
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::from("title=x&body_markdown=y"))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_forms_create_publish_edit_and_delete() {
    let (_, app) = app_with(&[]);
    let cookie = login(&app).await;

    let created = send(
        &app,
        form_post(
            "/admin/posts",
            &cookie,
            "title=Hello+World&excerpt=first&tags=Rust%2C+c%23&body_markdown=**hi**&status=draft",
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::SEE_OTHER);
    let edit_url = created.headers[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(edit_url.ends_with("/edit?saved=1"));
    assert_eq!(
        send(&app, get("/posts/hello-world")).await.status,
        StatusCode::NOT_FOUND
    );

    let id = edit_url.split('/').nth(3).unwrap().to_owned();
    let published = send(
        &app,
        form_post(
            &format!("/admin/posts/{id}"),
            &cookie,
            "title=Hello+World&tags=Rust%2C+c%23&body_markdown=**hi**&status=published",
        ),
    )
    .await;
    assert_eq!(published.status, StatusCode::SEE_OTHER);
    let page = send(&app, get("/posts/hello-world")).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("<strong>hi</strong>"));

    let editor = send(
        &app,
        Request::get(edit_url.as_str())
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(editor.body.contains("저장했습니다."));
    assert!(editor.body.contains("value=\"rust, c#\""));

    let deleted = send(
        &app,
        form_post(&format!("/admin/posts/{id}/delete"), &cookie, ""),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::SEE_OTHER);
    assert_eq!(
        send(&app, get("/posts/hello-world")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn admin_form_keeps_input_when_validation_fails() {
    let (_, app) = app_with(&[post("같은 제목", PostStatus::Draft, &[])]);
    let cookie = login(&app).await;
    let reply = send(
        &app,
        form_post(
            "/admin/posts",
            &cookie,
            "title=%21%21%21&body_markdown=%EB%82%B4%EC%9A%A9",
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert!(reply.body.contains("value=\"!!!\""));
    assert!(reply.body.contains("form-message error"));

    let conflict = send(
        &app,
        form_post(
            "/admin/posts",
            &cookie,
            "title=%EA%B0%99%EC%9D%80+%EC%A0%9C%EB%AA%A9&body_markdown=x",
        ),
    )
    .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn api_requires_authentication() {
    let (_, app) = app_with(&[]);
    let body = serde_json::to_string(&post("관리 글", PostStatus::Published, &[])).unwrap();
    let unauthorized = send(
        &app,
        Request::post("/api/admin/posts")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.clone()))
            .unwrap(),
    )
    .await;
    assert_eq!(unauthorized.status, StatusCode::UNAUTHORIZED);
    let wrong = send(
        &app,
        Request::post("/api/admin/posts")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer nope")
            .body(Body::from(body.clone()))
            .unwrap(),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    let created = send(&app, api("POST", "/api/admin/posts", &body)).await;
    assert_eq!(created.status, StatusCode::CREATED);
    let json: serde_json::Value = serde_json::from_str(&created.body).unwrap();
    assert_eq!(json["status"], "published");
    assert!(json.get("body_html").is_none());
}

#[tokio::test]
async fn api_session_cookie_also_works() {
    let (_, app) = app_with(&[]);
    let cookie = login(&app).await;
    let reply = send(
        &app,
        Request::get("/api/admin/posts")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, "[]");
}

#[tokio::test]
async fn api_rejects_bad_input_with_400_and_duplicates_with_409() {
    let (_, app) = app_with(&[]);
    for body in [
        "{not json",
        "{\"title\": \"제목만\"}",
        "{\"title\": \"\", \"body_markdown\": \"\"}",
        "{\"title\": \"!!!\", \"body_markdown\": \"본문\"}",
        "{\"title\": \"x\", \"body_markdown\": \"y\", \"status\": \"archived\"}",
    ] {
        let reply = send(&app, api("POST", "/api/admin/posts", body)).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            reply.body
        );
    }
    let body = "{\"title\": \"중복\", \"body_markdown\": \"본문\"}";
    assert_eq!(
        send(&app, api("POST", "/api/admin/posts", body))
            .await
            .status,
        StatusCode::CREATED
    );
    let duplicate = send(&app, api("POST", "/api/admin/posts", body)).await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn api_updates_and_deletes() {
    let (state, app) = app_with(&[post("원래", PostStatus::Draft, &[])]);
    let id = state.repository.get_by_slug("원래").unwrap().unwrap().id;
    let updated = send(
        &app,
        api(
            "PUT",
            &format!("/api/admin/posts/{id}"),
            "{\"title\": \"바뀜\", \"slug\": \"changed\", \"body_markdown\": \"본문\", \"status\": \"published\"}",
        ),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(
        send(&app, get("/posts/changed")).await.status,
        StatusCode::OK
    );
    let missing = send(
        &app,
        api(
            "PUT",
            "/api/admin/posts/9999",
            "{\"title\": \"x\", \"body_markdown\": \"y\"}",
        ),
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(
        send(&app, api("DELETE", &format!("/api/admin/posts/{id}"), ""))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, get("/posts/changed")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn stored_markdown_cannot_inject_scripts() {
    let mut input = post("XSS", PostStatus::Published, &[]);
    input.body_markdown = "<script>alert(1)</script>\n\n[x](javascript:alert(1))".to_owned();
    let (_, app) = app_with(&[input]);
    let reply = send(&app, get("/posts/xss")).await;
    assert!(!reply.body.contains("<script>alert"));
    assert!(!reply.body.contains("javascript:"));
}

#[tokio::test]
async fn tag_links_are_url_encoded_and_exact() {
    let (_, app) = app_with(&[
        post("샵", PostStatus::Published, &["c#", "a b"]),
        post("다른", PostStatus::Published, &["c"]),
    ]);
    let home = send(&app, get("/")).await;
    assert!(home.body.contains("href=\"/tags/c%23\""));
    assert!(home.body.contains("href=\"/tags/a%20b\""));
    let tag = send(&app, get("/tags/c%23")).await;
    assert_eq!(tag.status, StatusCode::OK);
    assert!(tag.body.contains("샵"));
    assert!(!tag.body.contains("다른"));
    assert_eq!(
        send(&app, get("/tags/none")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn search_only_returns_published_matches_and_escapes_wildcards() {
    let (_, app) = app_with(&[
        post("Rust 공개 검색", PostStatus::Published, &[]),
        post("Rust 초안 검색", PostStatus::Draft, &[]),
        post("100% 완료", PostStatus::Published, &[]),
    ]);
    let reply = send(&app, get("/search?q=Rust")).await;
    assert!(reply.body.contains("Rust 공개 검색"));
    assert!(!reply.body.contains("Rust 초안 검색"));
    let percent = send(&app, get("/search?q=%25")).await;
    assert!(percent.body.contains("100% 완료"));
    assert!(!percent.body.contains("Rust 공개 검색"));
    let empty = send(&app, get("/search")).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert!(!empty.body.contains("post-row"));
}

#[tokio::test]
async fn home_is_paginated() {
    let posts: Vec<_> = (0..3)
        .map(|index| post(&format!("글 {index}"), PostStatus::Published, &[]))
        .collect();
    let mut config = Config::new(TOKEN);
    config.page_size = 2;
    let state = AppState::with_config(config);
    for input in &posts {
        state.repository.create_post(input).unwrap();
    }
    let app = build_app(state);
    let first = send(&app, get("/")).await;
    assert_eq!(first.body.matches("class=\"post-row\"").count(), 2);
    assert!(first.body.contains("href=\"/?page=2\""));
    let second = send(&app, get("/?page=2")).await;
    assert_eq!(second.body.matches("class=\"post-row\"").count(), 1);
    assert!(second.body.contains("글 0"));
    assert_eq!(
        send(&app, get("/?page=3")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn rss_has_absolute_links_dates_and_no_drafts() {
    let (_, app) = app_with(&[
        post("공개 RSS", PostStatus::Published, &["rust"]),
        post("초안 RSS", PostStatus::Draft, &[]),
    ]);
    let reply = send(&app, get("/feed.xml")).await;
    assert_eq!(
        reply.headers[header::CONTENT_TYPE],
        "application/rss+xml; charset=utf-8"
    );
    assert!(reply
        .body
        .starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?><rss version=\"2.0\">"));
    assert!(reply.body.contains("공개 RSS"));
    assert!(!reply.body.contains("초안 RSS"));
    assert!(reply.body.contains("<link>https://blog.example/</link>"));
    assert!(reply
        .body
        .contains("<link>https://blog.example/posts/%EA%B3%B5%EA%B0%9C-rss</link>"));
    assert!(reply.body.contains("<guid isPermaLink=\"true\">"));
    assert!(reply.body.contains("<pubDate>"));
    assert!(reply.body.contains("+0900</pubDate>"));
    assert!(reply.body.contains("<description>"));
}

#[tokio::test]
async fn sitemap_uses_absolute_urls() {
    let (_, app) = app_with(&[post("지도", PostStatus::Published, &[])]);
    let reply = send(&app, get("/sitemap.xml")).await;
    assert!(reply.body.contains("<loc>https://blog.example/</loc>"));
    assert!(reply
        .body
        .contains("<loc>https://blog.example/posts/%EC%A7%80%EB%8F%84</loc>"));
    assert!(reply.body.contains("<lastmod>"));
}

#[tokio::test]
async fn pages_send_security_headers_and_etags() {
    let (_, app) = app_with(&[post("캐시", PostStatus::Published, &[])]);
    let first = send(&app, get("/")).await;
    assert!(first.headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .contains("default-src 'self'"));
    assert_eq!(first.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let etag = first.headers[header::ETAG].to_str().unwrap().to_owned();
    let cached = send(
        &app,
        Request::get("/")
            .header(header::IF_NONE_MATCH, &etag)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert!(cached.body.is_empty());
}

#[tokio::test]
async fn static_assets_are_embedded_and_versioned() {
    let (_, app) = app_with(&[]);
    let home = send(&app, get("/")).await;
    let start = home.body.find("/static/style.css?v=").unwrap();
    let url = &home.body[start..home.body[start..].find('"').unwrap() + start];
    let css = send(&app, get(url)).await;
    assert_eq!(css.status, StatusCode::OK);
    assert_eq!(css.headers[header::CONTENT_TYPE], "text/css; charset=utf-8");
    assert!(css.headers[header::CACHE_CONTROL]
        .to_str()
        .unwrap()
        .contains("immutable"));
    assert_eq!(
        send(&app, get("/static/missing.css")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn health_check_answers() {
    let (_, app) = app_with(&[]);
    let reply = send(&app, get("/healthz")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, "ok");
}
