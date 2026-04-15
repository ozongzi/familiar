use axum::{
    extract::{Query, State},
    response::Redirect,
};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::web::AppState;

#[derive(Deserialize)]
pub struct LoginQuery {
    pub client: Option<String>,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

/// GET /api/auth/github?client=tauri → redirect to GitHub authorization page
/// Pass client=tauri to get a familiar:// deep-link redirect after OAuth completes.
pub async fn github_login(
    State(state): State<AppState>,
    Query(params): Query<LoginQuery>,
) -> AppResult<Redirect> {
    if state.github_client_id.is_empty() {
        return Err(AppError::internal("GitHub OAuth not configured"));
    }
    let oauth_state = params.client.as_deref().unwrap_or("web").to_string();
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&scope=read:user&redirect_uri={}&state={}",
        state.github_client_id,
        urlencoding(&state.github_redirect_uri),
        urlencoding(&oauth_state),
    );
    Ok(Redirect::to(&url))
}

/// GET /api/auth/github/callback?code=... → exchange code, upsert user, redirect with token
pub async fn github_callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackQuery>,
) -> AppResult<Redirect> {
    let client = reqwest::Client::new();

    let token_resp: serde_json::Value = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id":     &state.github_client_id,
            "client_secret": &state.github_client_secret,
            "code":          &params.code,
            "redirect_uri":  &state.github_redirect_uri,
        }))
        .send()
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?
        .json()
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    let access_token = token_resp["access_token"]
        .as_str()
        .ok_or_else(|| AppError::bad_request("GitHub 授权失败"))?;

    let user_info: serde_json::Value = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "familiar-app")
        .send()
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?
        .json()
        .await
        .map_err(|e| AppError::internal(&e.to_string()))?;

    let github_id = user_info["id"]
        .as_i64()
        .ok_or_else(|| AppError::internal("GitHub 未返回用户 ID"))?
        .to_string();

    let login = user_info["login"].as_str().unwrap_or("gh_user");
    let display_name = user_info["name"].as_str().unwrap_or(login);

    let existing: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE github_id = $1")
            .bind(&github_id)
            .fetch_optional(&state.pool)
            .await?;

    let user_id = if let Some(id) = existing {
        sqlx::query("UPDATE users SET last_login_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&state.pool)
            .await?;
        id
    } else {
        let invite_code = gen_invite_code();
        let name = login.to_string();
        let inserted = sqlx::query_scalar::<_, uuid::Uuid>(
            "INSERT INTO users (name, github_id, display_name, invite_code) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(&name)
        .bind(&github_id)
        .bind(display_name)
        .bind(&invite_code)
        .fetch_optional(&state.pool)
        .await?;

        if let Some(id) = inserted {
            id
        } else {
            // Name conflict — append _gh
            sqlx::query_scalar::<_, uuid::Uuid>(
                "INSERT INTO users (name, github_id, display_name, invite_code) VALUES ($1, $2, $3, $4) RETURNING id",
            )
            .bind(format!("{}_gh", name))
            .bind(&github_id)
            .bind(display_name)
            .bind(gen_invite_code())
            .fetch_one(&state.pool)
            .await
            .map_err(|e| AppError::internal(&e.to_string()))?
        }
    };

    let token = generate_token();
    sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, NOW() + INTERVAL '30 days')",
    )
    .bind(&token)
    .bind(user_id)
    .execute(&state.pool)
    .await?;

    let _ = crate::audit::log_audit(
        &state.pool,
        Some(user_id),
        None,
        "github_login",
        Some(serde_json::json!({ "github_id": github_id, "login": login })),
        None,
    )
    .await;

    let is_new = existing.is_none();


    let is_tauri = params.state.as_deref() == Some("tauri");
    let redirect_url = if is_tauri {
        format!(
            "familiar://auth?token={}&is_new={}",
            token,
            if is_new { "1" } else { "0" }
        )
    } else {
        format!(
            "/#token={}&is_new={}",
            token,
            if is_new { "1" } else { "0" }
        )
    };
    Ok(Redirect::to(&redirect_url))
}

fn generate_token() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let mut s = String::with_capacity(64);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

pub fn gen_invite_code() -> String {
    use std::fmt::Write;
    let mut bytes = [0u8; 5];
    getrandom::getrandom(&mut bytes).expect("getrandom failed");
    let mut s = String::with_capacity(10);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf);
                let len = c.len_utf8();
                buf[..len]
                    .iter()
                    .flat_map(|b| format!("%{:02X}", b).chars().collect::<Vec<_>>())
                    .collect()
            }
        })
        .collect()
}
