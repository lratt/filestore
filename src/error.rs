use axum::{http::StatusCode, response::IntoResponse};
use tracing::warn;

pub struct AppError {
    pub inner: anyhow::Error,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        if let Some(sqlx::Error::RowNotFound) = self.inner.downcast_ref::<sqlx::Error>() {
            (StatusCode::NOT_FOUND, "Resource not found")
        } else {
            warn!(?self.inner, "unhandled internal error");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }
        .into_response()
    }
}

pub fn any_error<E: Into<anyhow::Error>>(e: E) -> AppError {
    AppError { inner: e.into() }
}
