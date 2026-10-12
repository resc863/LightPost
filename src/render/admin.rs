use maud::{html, Markup};

use super::{display_date, layout, post_path, PageMeta};
use crate::{
    config::Config,
    domain::{Post, PostForm, PostSummary},
};

pub fn login(config: &Config, error: Option<&str>) -> String {
    layout(
        config,
        &PageMeta::new("관리자 로그인").private(false),
        html! {
            section class="page-intro" {
                p class="eyebrow" { "ADMIN" }
                h1 { "관리자 로그인" }
                p class="intro-description" { "서버에 설정한 관리자 토큰을 입력하세요." }
            }
            form class="editor narrow" method="post" action="/admin/login" {
                (message(None, error))
                label {
                    "관리자 토큰"
                    input type="password" name="token" autocomplete="current-password" required autofocus;
                }
                div class="editor-actions" {
                    span {}
                    button type="submit" { "로그인 ↗" }
                }
            }
        },
    )
}

pub fn dashboard(config: &Config, posts: &[PostSummary]) -> String {
    layout(
        config,
        &PageMeta::new("관리자").private(true),
        html! {
            section class="page-intro" {
                p class="eyebrow" { "ADMIN" }
                h1 { "기록 관리" }
                (admin_bar())
            }
            div class="section-heading" {
                h2 { "모든 글" }
                span { (posts.len()) "개의 글" }
            }
            @if posts.is_empty() {
                div class="empty-state" {
                    span class="empty-mark" aria-hidden="true" { "✳" }
                    h2 { "아직 글이 없습니다." }
                    a href="/admin/posts/new" { "첫 글 쓰기 " span aria-hidden="true" { "↗" } }
                }
            }
            @for post in posts {
                article class="post-row admin-row" {
                    p class="post-date" { (display_date(config, &post.updated_at)) }
                    div class="post-summary" {
                        h3 { a href={ "/admin/posts/" (post.id) "/edit" } { (post.title) } }
                        p class="meta" {
                            span class={ "status status-" (post.status) } { (status_label(post.status.is_published())) }
                            @if post.status.is_published() {
                                " · " a href=(post_path(&post.slug)) { "공개 페이지 보기" }
                            }
                        }
                    }
                    span class="row-arrow" aria-hidden="true" { "↗" }
                }
            }
        },
    )
}

pub struct Editor<'a> {
    pub form: &'a PostForm,
    /// Set when editing an existing post.
    pub post: Option<&'a Post>,
    pub notice: Option<&'a str>,
    pub error: Option<&'a str>,
}

pub fn editor(config: &Config, editor: &Editor<'_>) -> String {
    let form = editor.form;
    let (title, action) = match editor.post {
        Some(post) => ("글 수정", format!("/admin/posts/{}", post.id)),
        None => ("새로운 기록", "/admin/posts".to_owned()),
    };
    layout(
        config,
        &PageMeta::new(title).private(true),
        html! {
            section class="page-intro" {
                p class="eyebrow" { "WRITE A NOTE" }
                h1 { (title) }
                (admin_bar())
            }
            form class="editor" method="post" action=(action) {
                (message(editor.notice, editor.error))
                label {
                    "제목"
                    input name="title" value=(form.title) placeholder="기록에 이름을 붙여주세요" required;
                }
                label {
                    "주소(slug) " span class="field-hint" { "비워 두면 제목으로 만들어요" }
                    input name="slug" value=(form.slug) placeholder="my-first-note";
                }
                label {
                    "요약 " span class="field-hint" { "목록과 RSS에 보여요" }
                    input name="excerpt" value=(form.excerpt);
                }
                label {
                    "태그 " span class="field-hint" { "쉼표로 구분해요" }
                    input name="tags" value=(form.tags) placeholder="rust, 일상";
                }
                label {
                    "본문 " span class="field-hint" { "Markdown으로 작성할 수 있어요" }
                    textarea name="body_markdown" placeholder="이곳에서 이야기를 시작하세요." required { (form.body_markdown) }
                }
                fieldset class="status-choice" {
                    legend { "공개 상태" }
                    label {
                        input type="radio" name="status" value="draft" checked[!form.is_published()];
                        " 초안"
                    }
                    label {
                        input type="radio" name="status" value="published" checked[form.is_published()];
                        " 발행"
                    }
                }
                div class="editor-actions" {
                    span class="meta" {
                        @if let Some(post) = editor.post {
                            @if post.status.is_published() {
                                a href=(post_path(&post.slug)) { "공개 페이지 보기 ↗" }
                            } @else {
                                "초안은 공개 목록에 나타나지 않아요."
                            }
                        } @else {
                            "초안은 공개 목록에 나타나지 않아요."
                        }
                    }
                    button type="submit" { "저장 ↗" }
                }
            }
            @if let Some(post) = editor.post {
                form class="danger-zone" method="post" action={ "/admin/posts/" (post.id) "/delete" }
                    data-confirm="이 글을 삭제할까요? 되돌릴 수 없습니다." {
                    button class="danger" type="submit" { "글 삭제" }
                }
            }
        },
    )
}

fn admin_bar() -> Markup {
    html! {
        div class="admin-bar" {
            a href="/admin" { "모든 글" }
            a href="/admin/posts/new" { "새 글 쓰기" }
            form method="post" action="/admin/logout" {
                button class="link-button" type="submit" { "로그아웃" }
            }
        }
    }
}

fn message(notice: Option<&str>, error: Option<&str>) -> Markup {
    html! {
        @if let Some(error) = error {
            p class="form-message error" role="alert" { (error) }
        }
        @if let Some(notice) = notice {
            p class="form-message notice" role="status" { (notice) }
        }
    }
}

fn status_label(published: bool) -> &'static str {
    if published {
        "발행됨"
    } else {
        "초안"
    }
}
