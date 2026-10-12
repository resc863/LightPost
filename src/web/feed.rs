use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};

use super::AppState;
use crate::{error::AppError, render};

const FEED_ITEMS: u32 = 20;

pub async fn rss(State(state): State<AppState>) -> Result<Response, AppError> {
    let posts = state
        .db(|repo| repo.published_summaries(Some(FEED_ITEMS)))
        .await?;
    Ok((
        [(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")],
        render::feed::rss(&state.config, &posts),
    )
        .into_response())
}

pub async fn sitemap(State(state): State<AppState>) -> Result<Response, AppError> {
    let posts = state.db(|repo| repo.published_summaries(None)).await?;
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        render::feed::sitemap(&state.config, &posts),
    )
        .into_response())
}
