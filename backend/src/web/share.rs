use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;
use crate::web::auth::AuthUser;
use crate::web::history::MessageResponse;

#[derive(Serialize)]
pub struct ShareResponse {
    pub token: Option<String>,
}

#[derive(Serialize)]
pub struct PublicSharedConversation {
    pub name: String,
    pub created_at: String,
    pub messages: Vec<MessageResponse>,
}

#[derive(Serialize)]
pub struct ImportResponse {
    pub conversation_id: Uuid,
}

fn gen_share_token() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let mut s = String::with_capacity(32);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

async fn assert_owner(state: &AppState, user_id: Uuid, conversation_id: Uuid) -> AppResult<()> {
    let owned: bool = sqlx::query_scalar::<_, Option<bool>>(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = $1 AND user_id = $2)",
    )
    .bind(conversation_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?
    .unwrap_or(false);
    if !owned {
        return Err(AppError::not_found("对话不存在"));
    }
    Ok(())
}

pub async fn get_share(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ShareResponse>> {
    assert_owner(&state, auth.user_id, id).await?;
    let token: Option<String> =
        sqlx::query_scalar("SELECT token FROM conversation_shares WHERE conversation_id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    Ok(Json(ShareResponse { token }))
}

pub async fn create_share(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ShareResponse>> {
    assert_owner(&state, auth.user_id, id).await?;

    if let Some(existing) = sqlx::query_scalar::<_, String>(
        "SELECT token FROM conversation_shares WHERE conversation_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    {
        return Ok(Json(ShareResponse {
            token: Some(existing),
        }));
    }

    let token = gen_share_token();
    sqlx::query(
        "INSERT INTO conversation_shares (token, conversation_id, created_by) VALUES ($1, $2, $3)",
    )
    .bind(&token)
    .bind(id)
    .bind(auth.user_id)
    .execute(&state.pool)
    .await?;

    Ok(Json(ShareResponse { token: Some(token) }))
}

pub async fn delete_share(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    assert_owner(&state, auth.user_id, id).await?;
    sqlx::query("DELETE FROM conversation_shares WHERE conversation_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Public: returns the conversation's name + active-branch messages for
/// anyone holding the share token. No auth required.
pub async fn get_shared_conversation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<PublicSharedConversation>> {
    let row = sqlx::query(
        r#"
        SELECT c.id AS conv_id, c.name AS conv_name, c.created_at AS conv_created
        FROM conversation_shares s
        JOIN conversations c ON c.id = s.conversation_id
        WHERE s.token = $1
        "#,
    )
    .bind(&token)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("分享链接无效或已被撤销"))?;

    let conv_id: Uuid = row
        .try_get("conv_id")
        .map_err(|_| AppError::internal("db error"))?;
    let name: String = row
        .try_get("conv_name")
        .map_err(|_| AppError::internal("db error"))?;
    let created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc> = row
        .try_get("conv_created")
        .map_err(|_| AppError::internal("db error"))?;

    let rows = state.db.list_active_with_siblings(conv_id).await?;

    let messages: Vec<MessageResponse> = rows
        .into_iter()
        .filter(|(r, _)| {
            // Public shares omit system-injected user turns (compaction
            // trigger, continue bridge, initial memory snapshot) — they carry
            // no timestamp, unlike messages the user actually typed.
            !matches!(
                (r.role.as_str(), r.content.as_deref()),
                ("user", Some(content)) if crate::db::is_synthetic_user_turn(content)
            )
        })
        .map(|(r, siblings)| MessageResponse {
            id: r.id,
            role: r.role,
            name: r.name,
            content: r.content,
            tool_calls: r.spell_casts,
            tool_call_id: r.spell_cast_id,
            created_at: r.created_at,
            streaming: r.streaming,
            reasoning: r.reasoning,
            parent_id: r.parent_id,
            siblings,
            // Synthetic turns are filtered out above, so nothing here is a note.
            note: None,
        })
        .collect();

    Ok(Json(PublicSharedConversation {
        name,
        created_at: created_at.to_rfc3339(),
        messages,
    }))
}

/// Authenticated: clone the shared conversation's active branch into a new
/// conversation owned by the caller. Returns the new conversation id so the
/// frontend can navigate the user into their own copy and keep chatting.
pub async fn import_share(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(token): Path<String>,
) -> AppResult<Json<ImportResponse>> {
    let source_conv: Uuid =
        sqlx::query_scalar("SELECT conversation_id FROM conversation_shares WHERE token = $1")
            .bind(&token)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("分享链接无效或已被撤销"))?;

    let source_name: String = sqlx::query_scalar("SELECT name FROM conversations WHERE id = $1")
        .bind(source_conv)
        .fetch_one(&state.pool)
        .await?;

    // Read the source's active branch up front (no transaction lock needed —
    // worst case is the source gets a new turn mid-import; the clone still
    // captures a coherent prefix snapshot).
    let rows = state.db.list_active_with_siblings(source_conv).await?;

    let mut tx = state.pool.begin().await?;

    let new_name = format!("{source_name} (导入)");
    let new_conv_id: Uuid = sqlx::query_scalar(
        "INSERT INTO conversations (user_id, name) VALUES ($1, $2) RETURNING id",
    )
    .bind(auth.user_id)
    .bind(&new_name)
    .fetch_one(&mut *tx)
    .await?;

    let base_now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut id_map: HashMap<i64, i64> = HashMap::new();
    let mut tail: Option<i64> = None;

    for (i, (r, _)) in rows.iter().enumerate() {
        let new_parent: Option<i64> = r.parent_id.and_then(|pid| id_map.get(&pid).copied());
        let new_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO messages
                (conversation_id, role, name, content, spell_casts, spell_cast_id,
                 reasoning, created_at, parent_id, streaming)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, FALSE)
            RETURNING id
            "#,
        )
        .bind(new_conv_id)
        .bind(&r.role)
        .bind(&r.name)
        .bind(&r.content)
        .bind(&r.spell_casts)
        .bind(&r.spell_cast_id)
        .bind(&r.reasoning)
        .bind(base_now + i as i64)
        .bind(new_parent)
        .fetch_one(&mut *tx)
        .await?;
        id_map.insert(r.id, new_id);
        tail = Some(new_id);
    }

    if let Some(t) = tail {
        sqlx::query("UPDATE conversations SET active_message_id = $1 WHERE id = $2")
            .bind(t)
            .bind(new_conv_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    Ok(Json(ImportResponse {
        conversation_id: new_conv_id,
    }))
}
