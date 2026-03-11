CREATE EXTENSION IF NOT EXISTS vector;

-- ── Users ─────────────────────────────────────────────────────────────────────

CREATE TABLE users (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT        NOT NULL UNIQUE,
    password_hash TEXT        NOT NULL,
    is_admin      BOOLEAN     NOT NULL DEFAULT false,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Sessions ──────────────────────────────────────────────────────────────────

CREATE TABLE sessions (
    token      TEXT        PRIMARY KEY,
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Conversations ─────────────────────────────────────────────────────────────

CREATE TABLE conversations (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT        NOT NULL DEFAULT '新对话',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Messages ──────────────────────────────────────────────────────────────────

CREATE TABLE messages (
    id              BIGSERIAL   PRIMARY KEY,
    conversation_id UUID        NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role            TEXT        NOT NULL,
    name            TEXT,
    content         TEXT,
    spell_casts     TEXT,
    spell_cast_id   TEXT,
    is_summary      BOOLEAN     NOT NULL DEFAULT false,
    created_at      BIGINT      NOT NULL,
    embedding       vector(1536),
    content_tsv     tsvector GENERATED ALWAYS AS (
                        to_tsvector('simple', COALESCE(content, ''))
                    ) STORED
);

CREATE INDEX idx_messages_conversation ON messages (conversation_id, id);
CREATE INDEX idx_messages_fts          ON messages USING GIN (content_tsv);
CREATE INDEX idx_messages_embedding    ON messages USING hnsw (embedding vector_cosine_ops);
