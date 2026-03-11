use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub struct AppError(pub StatusCode, pub Value);

impl AppError {
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, json!("未授权"))
    }

    pub fn not_found(msg: &str) -> Self {
        Self(StatusCode::NOT_FOUND, json!(msg))
    }

    pub fn bad_request(msg: &str) -> Self {
        Self(StatusCode::BAD_REQUEST, json!(msg))
    }

    pub fn internal(msg: &str) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, json!(msg))
    }

    pub fn conflict(msg: &str) -> Self {
        Self(StatusCode::CONFLICT, json!(msg))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!("sqlx error: {:?}", err);
        match err {
            sqlx::Error::RowNotFound => AppError::not_found("数据不存在"),
            sqlx::Error::Database(db_err) => {
                if db_err.constraint().is_some() {
                    AppError::conflict("数据已存在")
                } else {
                    AppError::internal("数据库错误")
                }
            }
            _ => AppError::internal("数据库错误"),
        }
    }
}

impl From<bcrypt::BcryptError> for AppError {
    fn from(err: bcrypt::BcryptError) -> Self {
        tracing::error!("bcrypt error: {:?}", err);
        AppError::internal("密码学错误")
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        tracing::error!("anyhow error: {:?}", err);
        AppError::internal(&err.to_string())
    }
}
