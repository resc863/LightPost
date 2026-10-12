use axum::{
    extract::{Path, Query, State},
    response::Html,
};
use serde::Deserialize;

use super::AppState;
use crate::{domain::Page, error::AppError, render};

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    page: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
    page: Option<u32>,
}

/// Page numbers past the end are 404s rather than empty lists.
fn check_range<T>(page: &Page<T>) -> Result<(), AppError> {
    if page.page > 1 && page.items.is_empty() {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn home(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Html<String>, AppError> {
    let per_page = state.config.page_size;
    let page = query.page.unwrap_or(1);
    let posts = state
        .db(move |repo| repo.list_published(page, per_page))
        .await?;
    check_range(&posts)?;
    Ok(Html(render::public::home(&state.config, &posts)))
}

pub async fn post_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let post = state
        .db(move |repo| repo.get_published_by_slug(&slug))
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Html(render::public::post(&state.config, &post)))
}

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Html<String>, AppError> {
    let per_page = state.config.page_size;
    let page = query.page.unwrap_or(1);
    let term = query.q.clone();
    let results = state
        .db(move |repo| repo.search_published(&term, page, per_page))
        .await?;
    check_range(&results)?;
    Ok(Html(render::public::search(
        &state.config,
        &query.q,
        &results,
    )))
}

pub async fn tag_page(
    State(state): State<AppState>,
    Path(tag): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Html<String>, AppError> {
    let per_page = state.config.page_size;
    let page = query.page.unwrap_or(1);
    let name = tag.clone();
    let posts = state
        .db(move |repo| repo.list_by_tag(&name, page, per_page))
        .await?;
    if posts.items.is_empty() {
        return Err(AppError::NotFound);
    }
    Ok(Html(render::public::tag(&state.config, &tag, &posts)))
}

pub async fn not_found() -> AppError {
    AppError::NotFound
}
