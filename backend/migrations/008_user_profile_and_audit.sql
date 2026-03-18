-- ── User Profile Extension ───────────────────────────────────────────────────

ALTER TABLE users ADD COLUMN email TEXT UNIQUE;
ALTER TABLE users ADD COLUMN display_name TEXT;
ALTER TABLE users ADD COLUMN avatar_path TEXT;
ALTER TABLE users ADD COLUMN last_login_at TIMESTAMPTZ;

-- Set default display_name to username for existing users
UPDATE users SET display_name = name WHERE display_name IS NULL;

-- ── Audit Logs ────────────────────────────────────────────────────────────────

CREATE TABLE audit_logs (
    id              BIGSERIAL   PRIMARY KEY,
    user_id         UUID        REFERENCES users(id) ON DELETE SET NULL,
    target_user_id  UUID        REFERENCES users(id) ON DELETE CASCADE,
    action          TEXT        NOT NULL,
    details         JSONB,
    ip_address      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for efficient querying
CREATE INDEX idx_audit_user ON audit_logs(user_id, created_at DESC);
CREATE INDEX idx_audit_target ON audit_logs(target_user_id, created_at DESC);
CREATE INDEX idx_audit_action ON audit_logs(action, created_at DESC);
CREATE INDEX idx_audit_created ON audit_logs(created_at DESC);
