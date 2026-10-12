//! LightPost: a small personal blog served by Axum from a single SQLite file.
//!
//! - `config`: settings read from `LIGHTPOST_*` environment variables
//! - `domain`: posts, tags, slugs and validation
//! - `db`: SQLite storage and schema migrations
//! - `markdown`: sanitized Markdown rendering
//! - `render`: HTML, RSS and sitemap templates
//! - `web`: routes, handlers, authentication and middleware

pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod markdown;
pub mod render;
pub mod time;
pub mod web;

pub use config::{Config, Site};
pub use db::Repository;
pub use domain::{Post, PostInput, PostStatus, PostSummary};
pub use error::AppError;
pub use markdown::render_markdown;
pub use web::{build_app, AppState};
