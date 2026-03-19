//! Audit log functionality for tracking user actions.

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct AuditLogWithNames {
    pub id: i64,
    pub user_id: Option<Uuid>,
    pub user_name: Option<String>,
    pub target_user_id: Option<Uuid>,
    pub target_user_name: Option<String>,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Log an audit event to the database.
pub async fn log_audit(
    pool: &PgPool,
    user_id: Option<Uuid>,
    target_user_id: Option<Uuid>,
    action: &str,
    details: Option<serde_json::Value>,
    ip_address: Option<String>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_logs (user_id, target_user_id, action, details, ip_address)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(user_id)
    .bind(target_user_id)
    .bind(action)
    .bind(details)
    .bind(ip_address)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub user_id: Option<Uuid>,
    pub target_user_id: Option<Uuid>,
    pub action: Option<String>,
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AuditLogPage {
    pub items: Vec<AuditLogWithNames>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

/// Query audit logs with filtering and pagination.
pub async fn query_audit_logs(
    pool: &PgPool,
    query: AuditLogQuery,
) -> anyhow::Result<AuditLogPage> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(30).clamp(1, 100);
    let offset = (page - 1) * per_page;

    // Build WHERE clauses
    let mut where_clauses = Vec::new();
    let mut bind_idx = 1;

    if query.user_id.is_some() {
        where_clauses.push(format!("al.user_id = ${}", bind_idx));
        bind_idx += 1;
    }
    if query.target_user_id.is_some() {
        where_clauses.push(format!("al.target_user_id = ${}", bind_idx));
        bind_idx += 1;
    }
    if query.action.is_some() {
        where_clauses.push(format!("al.action = ${}", bind_idx));
        bind_idx += 1;
    }
    if query.start_date.is_some() {
        where_clauses.push(format!("al.created_at >= ${}", bind_idx));
        bind_idx += 1;
    }
    if query.end_date.is_some() {
        where_clauses.push(format!("al.created_at <= ${}", bind_idx));
        bind_idx += 1;
    }

    let where_clause = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    // Count total
    let count_sql = format!(
        r#"
        SELECT COUNT(*) as count
        FROM audit_logs al
        {}
        "#,
        where_clause
    );

    let mut count_query = sqlx::query(&count_sql);
    if let Some(uid) = query.user_id {
        count_query = count_query.bind(uid);
    }
    if let Some(tuid) = query.target_user_id {
        count_query = count_query.bind(tuid);
    }
    if let Some(ref action) = query.action {
        count_query = count_query.bind(action);
    }
    if let Some(start) = query.start_date {
        count_query = count_query.bind(start);
    }
    if let Some(end) = query.end_date {
        count_query = count_query.bind(end);
    }

    let total: i64 = count_query
        .fetch_one(pool)
        .await?
        .try_get("count")
        .unwrap_or(0);

    // Fetch paginated results with user names
    let fetch_sql = format!(
        r#"
        SELECT 
            al.id,
            al.user_id,
            u1.name as user_name,
            al.target_user_id,
            u2.name as target_user_name,
            al.action,
            al.details,
            al.ip_address,
            al.created_at
        FROM audit_logs al
        LEFT JOIN users u1 ON al.user_id = u1.id
        LEFT JOIN users u2 ON al.target_user_id = u2.id
        {}
        ORDER BY al.created_at DESC
        LIMIT ${} OFFSET ${}
        "#,
        where_clause, bind_idx, bind_idx + 1
    );

    let mut fetch_query = sqlx::query(&fetch_sql);
    if let Some(uid) = query.user_id {
        fetch_query = fetch_query.bind(uid);
    }
    if let Some(tuid) = query.target_user_id {
        fetch_query = fetch_query.bind(tuid);
    }
    if let Some(ref action) = query.action {
        fetch_query = fetch_query.bind(action);
    }
    if let Some(start) = query.start_date {
        fetch_query = fetch_query.bind(start);
    }
    if let Some(end) = query.end_date {
        fetch_query = fetch_query.bind(end);
    }
    fetch_query = fetch_query.bind(per_page as i64).bind(offset as i64);

    let rows = fetch_query.fetch_all(pool).await?;

    let items = rows
        .into_iter()
        .map(|row| AuditLogWithNames {
            id: row.try_get("id").unwrap_or(0),
            user_id: row.try_get("user_id").ok(),
            user_name: row.try_get("user_name").ok(),
            target_user_id: row.try_get("target_user_id").ok(),
            target_user_name: row.try_get("target_user_name").ok(),
            action: row.try_get("action").unwrap_or_default(),
            details: row.try_get("details").ok(),
            ip_address: row.try_get("ip_address").ok(),
            created_at: row.try_get("created_at").unwrap_or_else(|_| chrono::Utc::now()),
        })
        .collect();

    Ok(AuditLogPage {
        items,
        total,
        page,
        per_page,
    })
}
