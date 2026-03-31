-- ── Models ────────────────────────────────────────────────────────────────────
-- A user-visible model registry. Scope:
--   'global' → visible to all users, managed by admin
--   'user'   → private to the owning user

CREATE TABLE models (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID        REFERENCES users(id) ON DELETE CASCADE,  -- NULL for global
    scope      TEXT        NOT NULL DEFAULT 'user' CHECK (scope IN ('global', 'user')),
    label      TEXT        NOT NULL,           -- display name, e.g. "Claude Sonnet"
    provider   TEXT        NOT NULL,           -- deepseek | openai | anthropic | gemini | ...
    model_name TEXT        NOT NULL,           -- e.g. "claude-sonnet-4-5"
    api_base   TEXT        NOT NULL DEFAULT '',
    api_key    TEXT        NOT NULL DEFAULT '',
    extra_body JSONB       NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX models_user_idx ON models (user_id);
CREATE INDEX models_scope_idx ON models (scope);

-- ── conversations.model_id ────────────────────────────────────────────────────

ALTER TABLE conversations
    ADD COLUMN model_id UUID REFERENCES models(id) ON DELETE SET NULL;
