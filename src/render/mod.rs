//! HTML and XML output. `maud` escapes every interpolated value unless it is
//! wrapped in `PreEscaped`, which is used only for Markdown that
//! `markdown::render_markdown` already sanitized.

pub mod admin;
pub mod feed;
pub mod public;

use maud::{html, Markup, DOCTYPE};

use crate::{config::Config, domain::Page, time::LocalTime, web::assets};

/// Per-page values for the shared layout.
pub struct PageMeta<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    /// Path of this page's canonical URL, e.g. `/posts/hello`.
    pub canonical_path: Option<String>,
    pub noindex: bool,
    pub script: bool,
}

impl<'a> PageMeta<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            description: None,
            canonical_path: None,
            noindex: false,
            script: false,
        }
    }

    pub fn canonical(mut self, path: impl Into<String>) -> Self {
        self.canonical_path = Some(path.into());
        self
    }

    pub fn description(mut self, description: &'a str) -> Self {
        if !description.trim().is_empty() {
            self.description = Some(description);
        }
        self
    }

    /// Admin and error pages: kept out of search engines, and admin pages load the script.
    pub fn private(mut self, script: bool) -> Self {
        self.noindex = true;
        self.script = script;
        self
    }
}

pub fn layout(config: &Config, meta: &PageMeta<'_>, content: Markup) -> String {
    let site = &config.site;
    let description = meta.description.unwrap_or(&site.description);
    html! {
        (DOCTYPE)
        html lang=(site.lang) {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width,initial-scale=1";
                meta name="theme-color" content="#faf9f6";
                title { (meta.title) " · " (site.title) }
                meta name="description" content=(description);
                @if meta.noindex {
                    meta name="robots" content="noindex";
                }
                @if let Some(path) = &meta.canonical_path {
                    link rel="canonical" href={ (config.base_url) (path) };
                    meta property="og:url" content={ (config.base_url) (path) };
                }
                meta property="og:title" content=(meta.title);
                meta property="og:description" content=(description);
                meta property="og:site_name" content=(site.title);
                link rel="stylesheet" href=(assets::style_url());
                link rel="icon" type="image/svg+xml" href=(assets::favicon_url());
                link rel="alternate" type="application/rss+xml" title={ (site.title) " RSS" } href="/feed.xml";
            }
            body {
                a class="skip-link" href="#main" { "본문으로 건너뛰기" }
                div class="site-shell" {
                    header class="site-header" {
                        a class="wordmark" href="/" { (site.title) span aria-hidden="true" { "." } }
                        nav aria-label="주 메뉴" {
                            a href="/" { "기록" }
                            a href="/search" { "검색" }
                            a class="rss-link" href="/feed.xml" { "RSS " span aria-hidden="true" { "↗" } }
                        }
                    }
                    main id="main" { (content) }
                    footer class="site-footer" {
                        div {
                            a class="footer-brand" href="/" { (site.title) "." }
                            p { (site.tagline) }
                        }
                        a href="/admin" { "글 쓰기 " span aria-hidden="true" { "↗" } }
                    }
                }
                @if meta.script {
                    script type="module" src=(assets::script_url()) {}
                }
            }
        }
    }
    .into_string()
}

/// A full page for an error status.
pub fn error_page(config: &Config, status: u16, message: &str) -> String {
    let heading = match status {
        404 => "찾을 수 없는 페이지",
        401 | 403 => "접근할 수 없는 페이지",
        400..=499 => "요청을 처리할 수 없습니다",
        _ => "잠시 문제가 생겼습니다",
    };
    layout(
        config,
        &PageMeta::new(heading).private(false),
        html! {
            div class="empty-state error-state" {
                span class="empty-mark" aria-hidden="true" { (status) }
                h2 { (heading) }
                p { (message) }
                a href="/" { "전체 기록 보기 " span aria-hidden="true" { "↗" } }
            }
        },
    )
}

/// Keeps line breaks from configured text such as the home headline.
pub fn multiline(text: &str) -> Markup {
    html! {
        @for (index, line) in text.lines().enumerate() {
            @if index > 0 { br; }
            (line)
        }
    }
}

pub fn display_date(config: &Config, timestamp: &str) -> String {
    LocalTime::from_sqlite(timestamp, config.utc_offset_minutes)
        .map(|time| time.display_date())
        .unwrap_or_else(|| timestamp.chars().take(10).collect())
}

/// Percent-encodes everything except RFC 3986 unreserved characters, so any
/// slug, tag or search term can be put in a path segment or query value.
pub fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn post_path(slug: &str) -> String {
    format!("/posts/{}", encode_component(slug))
}

pub fn tag_path(tag: &str) -> String {
    format!("/tags/{}", encode_component(tag))
}

/// Previous / next links. `base` is a path with an optional query, e.g. `/search?q=rust`.
pub fn pagination<T>(page: &Page<T>, base: &str) -> Markup {
    let link = |number: u32| -> String {
        if number <= 1 {
            base.to_owned()
        } else if base.contains('?') {
            format!("{base}&page={number}")
        } else {
            format!("{base}?page={number}")
        }
    };
    html! {
        @if page.total_pages() > 1 {
            nav class="pagination" aria-label="페이지" {
                @if page.has_previous() {
                    a rel="prev" href=(link(page.page - 1)) { "← 이전" }
                } @else {
                    span {}
                }
                span class="meta" { (page.page) " / " (page.total_pages()) }
                @if page.has_next() {
                    a rel="next" href=(link(page.page + 1)) { "다음 →" }
                } @else {
                    span {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_are_percent_encoded() {
        assert_eq!(encode_component("c#"), "c%23");
        assert_eq!(encode_component("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(encode_component("러"), "%EB%9F%AC");
        assert_eq!(encode_component("hello-rust_1.0~"), "hello-rust_1.0~");
    }

    #[test]
    fn layout_escapes_configured_text() {
        let mut config = Config::new("token");
        config.site.title = "<Blog>".to_owned();
        let page = layout(&config, &PageMeta::new("\"제목\""), html! {});
        assert!(page.contains("&lt;Blog&gt;"));
        assert!(page.contains("&quot;제목&quot;"));
        assert!(!page.contains("<Blog>"));
    }
}
