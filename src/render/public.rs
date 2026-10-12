use maud::{html, Markup, PreEscaped};

use super::{
    display_date, encode_component, layout, multiline, pagination, post_path, tag_path, PageMeta,
};
use crate::{
    config::Config,
    domain::{Page, Post, PostSummary},
};

pub fn home(config: &Config, posts: &Page<PostSummary>) -> String {
    let site = &config.site;
    layout(
        config,
        &PageMeta::new("기록").canonical("/"),
        html! {
            @if posts.page <= 1 {
                section class="intro" {
                    p class="eyebrow" { "A PERSONAL JOURNAL" }
                    h1 { (multiline(&site.headline)) }
                    p class="intro-description" { (multiline(&site.description)) }
                }
            }
            section aria-labelledby="posts-heading" {
                div class="section-heading" {
                    h2 id="posts-heading" { "최근 기록" }
                    span { (posts.total) "개의 글" }
                }
                (post_list(config, &posts.items))
                (pagination(posts, "/"))
            }
        },
    )
}

pub fn post(config: &Config, post: &Post) -> String {
    layout(
        config,
        &PageMeta::new(&post.title)
            .description(&post.excerpt)
            .canonical(post_path(&post.slug)),
        html! {
            article class="post-detail" {
                a class="back-link" href="/" { "← 전체 기록" }
                header class="article-heading" {
                    p class="eyebrow" { (display_date(config, post.date())) }
                    h1 { (post.title) }
                    (tags(&post.tags))
                }
                // Sanitized by `markdown::render_markdown` when the post was saved.
                div class="prose" { (PreEscaped(&post.body_html)) }
                div class="article-end" {
                    span aria-hidden="true" { "✳" }
                    a href="/" { "다른 기록 읽기 ↗" }
                }
            }
        },
    )
}

pub fn search(config: &Config, term: &str, results: &Page<PostSummary>) -> String {
    let term = term.trim();
    let base = if term.is_empty() {
        "/search".to_owned()
    } else {
        format!("/search?q={}", encode_component(term))
    };
    let mut meta = PageMeta::new("검색");
    if !term.is_empty() {
        // Result pages are endless variations; keep them out of the index.
        meta = meta.private(false);
    }
    layout(
        config,
        &meta,
        html! {
            section class="page-intro" {
                p class="eyebrow" { "EXPLORE THE JOURNAL" }
                h1 { "기록을 찾아서" }
                p class="intro-description" { "궁금한 단어나 주제로 지난 생각들을 찾아보세요." }
            }
            form class="search" action="/search" role="search" {
                label class="sr-only" for="search-query" { "검색어" }
                input id="search-query" type="search" name="q" value=(term) placeholder="어떤 기록을 찾고 있나요?";
                button type="submit" { "검색 " span aria-hidden="true" { "↗" } }
            }
            @if !term.is_empty() {
                div class="section-heading" {
                    h2 { "검색 결과" }
                    span { (results.total) "개의 글" }
                }
                (post_list(config, &results.items))
                (pagination(results, &base))
            }
        },
    )
}

pub fn tag(config: &Config, tag: &str, posts: &Page<PostSummary>) -> String {
    let title = format!("태그: {tag}");
    layout(
        config,
        &PageMeta::new(&title).canonical(tag_path(tag)),
        html! {
            section class="page-intro" {
                p class="eyebrow" { "COLLECTED NOTES" }
                h1 { "#" (tag) }
                p class="intro-description" { "하나의 주제로 이어지는 기록들." }
            }
            div class="section-heading" {
                h2 { "태그된 글" }
                span { (posts.total) "개의 글" }
            }
            (post_list(config, &posts.items))
            (pagination(posts, &tag_path(tag)))
        },
    )
}

fn post_list(config: &Config, posts: &[PostSummary]) -> Markup {
    html! {
        @if posts.is_empty() {
            div class="empty-state" {
                span class="empty-mark" aria-hidden="true" { "✳" }
                h2 { "아직 글이 없습니다." }
                p { "새로운 기록이 쌓이면 이곳에서 만나볼 수 있어요." }
                a href="/" { "전체 기록 보기 " span aria-hidden="true" { "↗" } }
            }
        }
        @for post in posts {
            article class="post-row" {
                p class="post-date" { (display_date(config, post.date())) }
                div class="post-summary" {
                    h3 { a href=(post_path(&post.slug)) { (post.title) } }
                    p { (post.excerpt) }
                    (tags(&post.tags))
                }
                span class="row-arrow" aria-hidden="true" { "↗" }
            }
        }
    }
}

fn tags(tags: &[String]) -> Markup {
    html! {
        div class="tags" {
            @for tag in tags {
                a href=(tag_path(tag)) { "#" (tag) }
            }
        }
    }
}
