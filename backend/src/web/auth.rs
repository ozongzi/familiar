use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

/// Authenticated user, extracted from `Authorization: Bearer <token>` header.
pub struct AuthUser {
    pub user_id: Uuid,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync + AsRef<PgPool>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AppError::unauthorized())?;

        let pool: &PgPool = state.as_ref();

        let row = sqlx::query(
            r#"
            SELECT s.user_id, u.is_admin
            FROM sessions s
            JOIN users u ON u.id = s.user_id
            WHERE s.token = $1
            "#,
        )
        .bind(bearer.token())
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            tracing::error!("session lookup error: {e}");
            AppError::internal("数据库错误")
        })?
        .ok_or_else(AppError::unauthorized)?;

        use sqlx::Row;
        Ok(AuthUser {
            user_id: row
                .try_get("user_id")
                .map_err(|_| AppError::unauthorized())?,
        })
    }
}
