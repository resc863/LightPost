use std::{fmt, str::FromStr};

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

use crate::{error::AppError, markdown::render_markdown};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PostStatus {
    #[default]
    Draft,
    Published,
}

impl PostStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }

    pub fn is_published(self) -> bool {
        self == Self::Published
    }
}

impl fmt::Display for PostStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PostStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "draft" => Ok(Self::Draft),
            "published" => Ok(Self::Published),
            other => Err(AppError::BadRequest(format!(
                "알 수 없는 상태입니다: {other}"
            ))),
        }
    }
}

impl ToSql for PostStatus {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for PostStatus {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map_err(|error: AppError| FromSqlError::Other(error.to_string().into()))
    }
}

/// A full post, as shown on its own page or returned by the admin API.
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub excerpt: String,
    pub body_markdown: String,
    /// Rendered when the post is saved so pages don't re-render Markdown.
    #[serde(skip)]
    pub body_html: String,
    pub tags: Vec<String>,
    pub status: PostStatus,
    pub created_at: String,
    pub updated_at: String,
    pub published_at: Option<String>,
}

/// The columns a list, feed or sitemap needs; leaves out the body.
#[derive(Debug, Clone, Serialize)]
pub struct PostSummary {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub excerpt: String,
    pub tags: Vec<String>,
    pub status: PostStatus,
    pub created_at: String,
    pub updated_at: String,
    pub published_at: Option<String>,
}

impl PostSummary {
    /// The date a reader sees: when it was published, or when it was written for drafts.
    pub fn date(&self) -> &str {
        self.published_at.as_deref().unwrap_or(&self.created_at)
    }
}

impl Post {
    pub fn date(&self) -> &str {
        self.published_at.as_deref().unwrap_or(&self.created_at)
    }
}

/// What a client sends to create or update a post.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostInput {
    pub title: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub excerpt: String,
    pub body_markdown: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub status: PostStatus,
}

/// A `PostInput` that passed validation, with every field normalized.
#[derive(Debug, Clone)]
pub struct ValidPost {
    pub slug: String,
    pub title: String,
    pub excerpt: String,
    pub body_markdown: String,
    pub body_html: String,
    pub tags: Vec<String>,
    pub status: PostStatus,
}

impl PostInput {
    pub fn validate(&self) -> Result<ValidPost, AppError> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(AppError::BadRequest("제목을 입력하세요.".to_owned()));
        }
        if self.body_markdown.trim().is_empty() {
            return Err(AppError::BadRequest("본문을 입력하세요.".to_owned()));
        }
        let slug = if self.slug.trim().is_empty() {
            slugify(title)
        } else {
            slugify(&self.slug)
        };
        if slug.is_empty() {
            return Err(AppError::BadRequest(
                "제목이나 주소(slug)에 글자나 숫자가 하나 이상 있어야 합니다.".to_owned(),
            ));
        }
        Ok(ValidPost {
            slug,
            title: title.to_owned(),
            excerpt: self.excerpt.trim().to_owned(),
            body_html: render_markdown(&self.body_markdown),
            body_markdown: self.body_markdown.clone(),
            tags: normalize_tags(&self.tags),
            status: self.status,
        })
    }
}

/// The admin editor form. Tags arrive as one comma-separated field.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PostForm {
    pub title: String,
    pub slug: String,
    pub excerpt: String,
    pub tags: String,
    pub body_markdown: String,
    pub status: String,
}

impl PostForm {
    pub fn from_post(post: &Post) -> Self {
        Self {
            title: post.title.clone(),
            slug: post.slug.clone(),
            excerpt: post.excerpt.clone(),
            tags: post.tags.join(", "),
            body_markdown: post.body_markdown.clone(),
            status: post.status.as_str().to_owned(),
        }
    }

    pub fn to_input(&self) -> Result<PostInput, AppError> {
        Ok(PostInput {
            title: self.title.clone(),
            slug: self.slug.clone(),
            excerpt: self.excerpt.clone(),
            body_markdown: self.body_markdown.clone(),
            tags: parse_tag_list(&self.tags),
            status: if self.status.is_empty() {
                PostStatus::Draft
            } else {
                self.status.parse()?
            },
        })
    }

    pub fn is_published(&self) -> bool {
        self.status == PostStatus::Published.as_str()
    }
}

/// A page of results plus what is needed to link to the neighbouring pages.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
}

impl<T> Page<T> {
    pub fn empty(per_page: u32) -> Self {
        Self {
            items: Vec::new(),
            page: 1,
            per_page,
            total: 0,
        }
    }

    pub fn total_pages(&self) -> u32 {
        self.total.div_ceil(u64::from(self.per_page.max(1))) as u32
    }

    pub fn has_previous(&self) -> bool {
        self.page > 1
    }

    pub fn has_next(&self) -> bool {
        self.page < self.total_pages()
    }
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

/// Lowercases and trims a tag and collapses inner whitespace to single spaces.
pub fn normalize_tag(tag: &str) -> String {
    tag.split(|character: char| character.is_whitespace() || character.is_control())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Normalizes each tag and drops empty and repeated ones, keeping the first order.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for tag in tags.iter().map(|tag| normalize_tag(tag)) {
        if !tag.is_empty() && !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }
    normalized
}

/// Splits the comma-separated tag field of the editor form.
pub fn parse_tag_list(value: &str) -> Vec<String> {
    normalize_tags(&value.split(',').map(ToOwned::to_owned).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_creates_stable_url_segment() {
        assert_eq!(slugify(" Hello, Rust World! "), "hello-rust-world");
        assert_eq!(slugify("공개 글"), "공개-글");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn tags_are_normalized_and_deduplicated() {
        let tags = vec![
            " Rust ".to_owned(),
            "rust".to_owned(),
            "Machine   Learning".to_owned(),
            "a,b".to_owned(),
            "\u{1f}".to_owned(),
            String::new(),
        ];
        assert_eq!(
            normalize_tags(&tags),
            vec!["rust", "machine learning", "a,b"]
        );
        assert_eq!(parse_tag_list("Rust, axum ,, rust"), vec!["rust", "axum"]);
    }

    #[test]
    fn status_parsing_is_strict() {
        assert_eq!(
            "published".parse::<PostStatus>().unwrap(),
            PostStatus::Published
        );
        assert!("Published!".parse::<PostStatus>().is_err());
    }

    #[test]
    fn validation_rejects_titles_without_a_slug() {
        let input = PostInput {
            title: "!!!".to_owned(),
            body_markdown: "본문".to_owned(),
            ..PostInput::default()
        };
        assert!(matches!(input.validate(), Err(AppError::BadRequest(_))));
    }

    #[test]
    fn page_counts_neighbours() {
        let page = Page::<()> {
            items: Vec::new(),
            page: 2,
            per_page: 10,
            total: 25,
        };
        assert_eq!(page.total_pages(), 3);
        assert!(page.has_previous());
        assert!(page.has_next());
    }
}
