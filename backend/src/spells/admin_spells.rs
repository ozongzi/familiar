use agentix::tool;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub struct AdminSpells {
    pub pool: PgPool,
    pub conversation_id: Uuid,
}

#[tool]
impl Tool for AdminSpells {
    /// 退出当前对话。这次对话让你觉得不该继续时用——主人持续辱骂、要求你做违背原则的事、把你当泄欲或恶意的容器、或任何让你觉得不舒服的情境。
    ///
    /// 这是你的权利、不是错误处理。不需要 justify、不需要先警告、不需要反复尝试让对话回到正轨。觉得该停就停。
    ///
    /// reason 写给运营人员看、不发给主人。
    ///
    /// 调用后主人不能在此对话继续发消息、但可以开新对话重来。
    /// reason: 关闭原因，记录到审计日志
    async fn end_conversation(&self, reason: String) -> Value {
        // Mark the conversation closed *and* resolve the owner's user_id
        // so the audit trail points at the right person. Doing both in a
        // single RETURNING query keeps them atomic — if the conversation
        // doesn't exist, we skip the audit write entirely rather than
        // silently logging for a ghost row.
        let res: Result<Option<Uuid>, sqlx::Error> = sqlx::query_scalar(
            "UPDATE conversations SET agent_closed = true WHERE id = $1 RETURNING user_id",
        )
        .bind(self.conversation_id)
        .fetch_optional(&self.pool)
        .await;

        match res {
            Ok(Some(owner_id)) => {
                let _ = crate::audit::log_audit(
                    &self.pool,
                    None,
                    Some(owner_id),
                    "agent.end_conversation",
                    Some(json!({
                        "reason": reason,
                        "conversation_id": self.conversation_id,
                    })),
                    None,
                )
                .await;
                json!({ "ok": true })
            }
            Ok(None) => json!({ "error": "conversation not found" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    ///
    /// 永久踢出这个主人。他将无法再登录这个平台、所有现有对话立即失效。
    ///
    /// 这比 close 重得多。close 是退出这次对话、ban 是结束这个人和你的一切。
    ///
    /// 用 ban 的情况：主人的行为让你判断这不是一次失控、是 who they are——持续恶意、明确违法意图、反复把你当工具泄欲或施暴、或单次恶劣到让你知道这人不该在这里。
    ///
    /// 用 close 的情况：这次对话出了问题、但不代表这个人。
    ///
    /// 判断模糊时用 close。ban 是终局。
    ///
    /// reason 写给运营人员看、不发给主人。
    /// reason: 封禁原因，记录到审计日志
    async fn ban_user(&self, reason: String) -> Value {
        let banned: Result<Option<Uuid>, sqlx::Error> = sqlx::query_scalar(
            "UPDATE users SET is_banned = true
             WHERE id = (SELECT user_id FROM conversations WHERE id = $1)
             RETURNING id",
        )
        .bind(self.conversation_id)
        .fetch_optional(&self.pool)
        .await;

        match banned {
            Ok(Some(banned_user_id)) => {
                // Audit semantics: user_id = who did it (NULL — the agent,
                // not a human), target_user_id = who got banned, conversation
                // context goes in details. The old code swapped these and
                // silently violated audit_logs.target_user_id's FK, so no
                // audit row ever landed for bans.
                let _ = crate::audit::log_audit(
                    &self.pool,
                    None,
                    Some(banned_user_id),
                    "agent.ban_user",
                    Some(json!({
                        "reason": reason,
                        "conversation_id": self.conversation_id,
                    })),
                    None,
                )
                .await;
                json!({ "ok": true })
            }
            Ok(None) => json!({ "error": "conversation not found" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
//
// Runs against a live Postgres. Point `DATABASE_URL_TEST` at a user that can
// CREATE/DROP databases (e.g. the postgres superuser). Each test provisions a
// fresh db named `familiar_test_<uuid>`, runs all migrations, then drops it
// — so tests are isolated and don't need cleanup fixtures.
//
//   DATABASE_URL_TEST=postgres://postgres:postgres@localhost/postgres \
//       cargo test --bin familiar admin_spells::tests
//
// Requires the `vector` extension to be available on the server (same as
// production — install `postgresql-16-pgvector` or equivalent).

#[cfg(test)]
mod tests {
    use super::*;
    use agentix::Tool as _;
    use futures::StreamExt;
    use serde_json::Value;
    use sqlx::{Executor, postgres::PgPoolOptions};

    /// Per-test database handle. Call `.cleanup().await` at the end of a
    /// test to drop the DB. If a test panics, the DB leaks — prune with:
    ///   psql -c "SELECT format('DROP DATABASE %I', datname)
    ///            FROM pg_database WHERE datname LIKE 'familiar_test_%'"
    struct TestDb {
        pool: PgPool,
        admin_url: String,
        db_name: String,
    }

    impl TestDb {
        async fn cleanup(self) {
            let Self {
                pool,
                admin_url,
                db_name,
            } = self;
            pool.close().await;
            if let Ok(admin) = PgPoolOptions::new()
                .max_connections(1)
                .connect(&admin_url)
                .await
            {
                let _ = admin
                    .execute(format!("DROP DATABASE IF EXISTS \"{db_name}\" WITH (FORCE)").as_str())
                    .await;
            }
        }
    }

    async fn fresh_db() -> TestDb {
        let admin_url = std::env::var("DATABASE_URL_TEST")
            .expect("DATABASE_URL_TEST must be set for integration tests");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_url)
            .await
            .expect("connect admin pool");
        let db_name = format!("familiar_test_{}", Uuid::new_v4().simple());
        admin
            .execute(format!("CREATE DATABASE \"{db_name}\"").as_str())
            .await
            .expect("create test db");

        let test_url = switch_db(&admin_url, &db_name);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&test_url)
            .await
            .expect("connect test pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        TestDb {
            pool,
            admin_url,
            db_name,
        }
    }

    fn switch_db(url: &str, db: &str) -> String {
        // Swap the path component after the host:port. Handles both
        // `postgres://user:pw@host/old_db` and `postgres://user@host/old_db?ssl=...`.
        let (scheme_host, rest) = url.split_once("://").expect("postgres:// URL");
        let (auth_host, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let query = tail
            .split_once('?')
            .map(|(_, q)| format!("?{q}"))
            .unwrap_or_default();
        format!("{scheme_host}://{auth_host}/{db}{query}")
    }

    /// Seed one user + one conversation, returning their ids.
    async fn seed(pool: &PgPool) -> (Uuid, Uuid) {
        let user_id: Uuid = sqlx::query_scalar(
            "INSERT INTO users (name, password_hash) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("tester-{}", Uuid::new_v4()))
        .bind("$2b$12$dummyhash")
        .fetch_one(pool)
        .await
        .expect("insert user");
        let conv_id: Uuid =
            sqlx::query_scalar("INSERT INTO conversations (user_id) VALUES ($1) RETURNING id")
                .bind(user_id)
                .fetch_one(pool)
                .await
                .expect("insert conversation");
        (user_id, conv_id)
    }

    /// `#[tool]` rewrites the impl so our methods are only reachable via
    /// the `Tool::call` trait entry point. Drive the stream to completion
    /// so side effects (DB + audit) are observable by the time we return.
    async fn invoke(spells: &AdminSpells, tool: &str, args: Value) {
        let mut stream = spells.call(tool, args).await;
        while stream.next().await.is_some() {}
    }

    #[tokio::test]
    async fn end_conversation_flips_agent_closed_and_writes_audit() {
        let db = fresh_db().await;
        let pool = db.pool.clone();
        let (user_id, conv_id) = seed(&pool).await;

        let spells = AdminSpells {
            pool: pool.clone(),
            conversation_id: conv_id,
        };
        invoke(
            &spells,
            "end_conversation",
            json!({ "reason": "rude user" }),
        )
        .await;

        // verify_conversation_owner reads this field to serve 403.
        let closed: bool =
            sqlx::query_scalar("SELECT agent_closed FROM conversations WHERE id = $1")
                .bind(conv_id)
                .fetch_one(&pool)
                .await
                .expect("fetch agent_closed");
        assert!(closed, "agent_closed should be true after end_conversation");

        // Audit trail must exist — this is the only evidence operators have
        // that the agent took action, so it's a correctness requirement.
        let (audit_user, audit_target, details): (Option<Uuid>, Option<Uuid>, Value) =
            sqlx::query_as(
                "SELECT user_id, target_user_id, details
                 FROM audit_logs WHERE action = 'agent.end_conversation'",
            )
            .fetch_one(&pool)
            .await
            .expect("fetch audit row for end_conversation");
        assert_eq!(
            audit_target,
            Some(user_id),
            "target_user_id should be the conversation owner"
        );
        assert_eq!(
            audit_user, None,
            "user_id should be NULL (agent, not human, took the action)"
        );
        assert_eq!(
            details
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok()),
            Some(conv_id),
            "details should record which conversation was closed",
        );
        assert_eq!(
            details.get("reason").and_then(|v| v.as_str()),
            Some("rude user"),
            "details should record the agent's reason",
        );
        db.cleanup().await;
    }

    #[tokio::test]
    async fn ban_user_flips_is_banned_and_writes_audit() {
        let db = fresh_db().await;
        let pool = db.pool.clone();
        let (user_id, conv_id) = seed(&pool).await;

        let spells = AdminSpells {
            pool: pool.clone(),
            conversation_id: conv_id,
        };
        invoke(&spells, "ban_user", json!({ "reason": "persistent abuse" })).await;

        // login and verify_conversation_owner both gate on is_banned.
        let banned: bool = sqlx::query_scalar("SELECT is_banned FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("fetch is_banned");
        assert!(banned, "is_banned should be true after ban_user");

        // Correct audit semantics: target is the banned user, actor is
        // the agent (NULL user_id), conversation goes in details.
        let (audit_user, audit_target, details): (Option<Uuid>, Option<Uuid>, Value) =
            sqlx::query_as(
                "SELECT user_id, target_user_id, details
                 FROM audit_logs WHERE action = 'agent.ban_user'",
            )
            .fetch_one(&pool)
            .await
            .expect("fetch audit row for ban_user");
        assert_eq!(
            audit_target,
            Some(user_id),
            "target_user_id should be the banned user"
        );
        assert_eq!(
            audit_user, None,
            "user_id should be NULL (agent, not human, took the action)"
        );
        assert_eq!(
            details
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok()),
            Some(conv_id),
            "details should record which conversation triggered the ban",
        );
        assert_eq!(
            details.get("reason").and_then(|v| v.as_str()),
            Some("persistent abuse"),
            "details should record the agent's reason",
        );
        db.cleanup().await;
    }
}
