//! Admin screens (plain HTML forms that work without JavaScript) and the JSON API.

use axum::{
    extract::{rejection::JsonRejection, Form, Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use serde::Deserialize;

use super::{
    auth::{AdminApi, AdminPage},
    AppState,
};
use crate::{
    domain::{Post, PostForm, PostInput, PostSummary},
    error::AppError,
    render::{self, admin::Editor},
};

#[derive(Debug, Deserialize)]
pub struct EditQuery {
    saved: Option<String>,
}

pub async fn dashboard(
    _: AdminPage,
    State(state): State<AppState>,
) -> Result<Html<String>, AppError> {
    let posts = state.db(|repo| repo.list_all()).await?;
    Ok(Html(render::admin::dashboard(&state.config, &posts)))
}

pub async fn new_post(_: AdminPage, State(state): State<AppState>) -> Html<String> {
    let form = PostForm::default();
    Html(editor_page(&state, &form, None, None, None))
}

pub async fn create_post(
    _: AdminPage,
    State(state): State<AppState>,
    Form(form): Form<PostForm>,
) -> Result<Response, AppError> {
    let result = match form.to_input() {
        Ok(input) => state.db(move |repo| repo.create_post(&input)).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(post) => {
            Ok(Redirect::to(&format!("/admin/posts/{}/edit?saved=1", post.id)).into_response())
        }
        Err(error) => form_error(&state, &form, None, error),
    }
}

pub async fn edit_post(
    _: AdminPage,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<EditQuery>,
) -> Result<Html<String>, AppError> {
    let post = state
        .db(move |repo| repo.get_by_id(id))
        .await?
        .ok_or(AppError::NotFound)?;
    let notice = query.saved.is_some().then_some("저장했습니다.");
    Ok(Html(editor_page(
        &state,
        &PostForm::from_post(&post),
        Some(&post),
        notice,
        None,
    )))
}

pub async fn update_post(
    _: AdminPage,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<PostForm>,
) -> Result<Response, AppError> {
    let result = match form.to_input() {
        Ok(input) => state.db(move |repo| repo.update_post(id, &input)).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(post) => {
            Ok(Redirect::to(&format!("/admin/posts/{}/edit?saved=1", post.id)).into_response())
        }
        Err(error) => {
            let post = state
                .db(move |repo| repo.get_by_id(id))
                .await?
                .ok_or(AppError::NotFound)?;
            form_error(&state, &form, Some(&post), error)
        }
    }
}

pub async fn delete_post(
    _: AdminPage,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    state.db(move |repo| repo.delete_post(id)).await?;
    Ok(Redirect::to("/admin"))
}

fn editor_page(
    state: &AppState,
    form: &PostForm,
    post: Option<&Post>,
    notice: Option<&str>,
    error: Option<&str>,
) -> String {
    render::admin::editor(
        &state.config,
        &Editor {
            form,
            post,
            notice,
            error,
        },
    )
}

/// Shows validation problems next to what the author typed instead of losing it.
fn form_error(
    state: &AppState,
    form: &PostForm,
    post: Option<&Post>,
    error: AppError,
) -> Result<Response, AppError> {
    match error {
        AppError::BadRequest(_) | AppError::Conflict(_) => {
            let page = editor_page(state, form, post, None, Some(&error.public_message()));
            Ok((error.status(), Html(page)).into_response())
        }
        other => Err(other),
    }
}

/// Every JSON problem (syntax, missing field, wrong content type) is a 400 with a message.
fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, AppError> {
    body.map(|Json(value)| value)
        .map_err(|rejection| AppError::BadRequest(rejection.body_text()))
}

pub async fn api_list(
    _: AdminApi,
    State(state): State<AppState>,
) -> Result<Json<Vec<PostSummary>>, AppError> {
    Ok(Json(state.db(|repo| repo.list_all()).await?))
}

pub async fn api_get(
    _: AdminApi,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Post>, AppError> {
    let post = state.db(move |repo| repo.get_by_id(id)).await?;
    Ok(Json(post.ok_or(AppError::NotFound)?))
}

pub async fn api_create(
    _: AdminApi,
    State(state): State<AppState>,
    body: Result<Json<PostInput>, JsonRejection>,
) -> Result<Response, AppError> {
    let input = json_body(body)?;
    let post = state.db(move |repo| repo.create_post(&input)).await?;
    Ok((StatusCode::CREATED, Json(post)).into_response())
}

pub async fn api_update(
    _: AdminApi,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Result<Json<PostInput>, JsonRejection>,
) -> Result<Json<Post>, AppError> {
    let input = json_body(body)?;
    Ok(Json(
        state.db(move |repo| repo.update_post(id, &input)).await?,
    ))
}

pub async fn api_delete(
    _: AdminApi,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    state.db(move |repo| repo.delete_post(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
