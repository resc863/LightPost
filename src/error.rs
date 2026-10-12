use std::fmt;

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

/// Every failure a handler or the repository can report.
///
/// The variants map one-to-one onto HTTP status codes, so handlers can use `?`
/// and still answer with the right status.
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict(String),
    /// The detail is logged on the server and never sent to the client.
    Internal(String),
}

impl AppError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The message that is safe to show to a visitor.
    pub fn public_message(&self) -> String {
        match self {
            Self::BadRequest(message) | Self::Conflict(message) => message.clone(),
            Self::Unauthorized => "로그인이 필요합니다.".to_owned(),
            Self::Forbidden => "허용되지 않은 요청입니다.".to_owned(),
            Self::NotFound => "페이지를 찾을 수 없습니다.".to_owned(),
            Self::Internal(_) => "서버에서 문제가 발생했습니다.".to_owned(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(detail) => write!(f, "internal error: {detail}"),
            other => f.write_str(&other.public_message()),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(failure, _) = &error {
            if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE {
                return Self::Conflict("같은 주소(slug)를 쓰는 글이 이미 있습니다.".to_owned());
            }
        }
        Self::Internal(error.to_string())
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Internal(error.to_string())
    }
}

/// Attached to error responses so the HTML layer can wrap them in the site layout.
#[derive(Debug, Clone)]
pub struct ErrorPage {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(detail) = &self {
            eprintln!("internal error: {detail}");
        }
        let status = self.status();
        let message = self.public_message();
        let mut response = (
            status,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            message.clone(),
        )
            .into_response();
        response
            .extensions_mut()
            .insert(ErrorPage { status, message });
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn internal_error_detail_is_not_sent_to_client() {
        let response =
            AppError::Internal("disk I/O error at /secret/path".to_owned()).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body.contains("/secret/path"));
    }
}
