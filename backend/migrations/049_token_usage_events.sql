CREATE TABLE IF NOT EXISTS token_usage_events (
    id                    BIGSERIAL PRIMARY KEY,
    job_id                UUID        REFERENCES generation_jobs(id) ON DELETE CASCADE,
    conversation_id       UUID        NOT NULL,
    user_id               UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    message_id            BIGINT      REFERENCES messages(id) ON DELETE SET NULL,
    conversation_name     TEXT,
    prompt_tokens         BIGINT      NOT NULL DEFAULT 0,
    completion_tokens     BIGINT      NOT NULL DEFAULT 0,
    cache_read_tokens     BIGINT      NOT NULL DEFAULT 0,
    cache_creation_tokens BIGINT      NOT NULL DEFAULT 0,
    total_tokens          BIGINT      NOT NULL DEFAULT 0,
    created_at            BIGINT      NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_token_usage_events_created_at
    ON token_usage_events (created_at);

CREATE INDEX IF NOT EXISTS idx_token_usage_events_user_created
    ON token_usage_events (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_token_usage_events_conv_created
    ON token_usage_events (conversation_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_token_usage_events_message
    ON token_usage_events (message_id);

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS context_tokens BIGINT;

-- Backfill one event per historical assistant message that has token data.
INSERT INTO token_usage_events (
    job_id,
    conversation_id,
    user_id,
    message_id,
    conversation_name,
    prompt_tokens,
    completion_tokens,
    cache_read_tokens,
    cache_creation_tokens,
    total_tokens,
    created_at
)
SELECT NULL,
       m.conversation_id,
       c.user_id,
       m.id,
       c.name,
       COALESCE(m.prompt_tokens, 0),
       COALESCE(m.completion_tokens, 0),
       COALESCE(m.cache_read_tokens, 0),
       COALESCE(m.cache_creation_tokens, 0),
       COALESCE(m.prompt_tokens, 0)
         + COALESCE(m.completion_tokens, 0)
         + COALESCE(m.cache_read_tokens, 0)
         + COALESCE(m.cache_creation_tokens, 0),
       m.created_at
FROM messages m
JOIN conversations c ON c.id = m.conversation_id
WHERE m.role = 'assistant'
  AND (
      m.prompt_tokens IS NOT NULL
      OR m.completion_tokens IS NOT NULL
      OR m.cache_read_tokens IS NOT NULL
      OR m.cache_creation_tokens IS NOT NULL
  );

-- Recompute per-message context snapshot used by compact.
UPDATE messages
SET context_tokens =
    COALESCE(prompt_tokens, 0)
  + COALESCE(cache_read_tokens, 0)
  + COALESCE(cache_creation_tokens, 0)
WHERE role = 'assistant'
  AND (
      prompt_tokens IS NOT NULL
      OR cache_read_tokens IS NOT NULL
      OR cache_creation_tokens IS NOT NULL
  );

ALTER TABLE messages
    DROP COLUMN IF EXISTS prompt_tokens,
    DROP COLUMN IF EXISTS completion_tokens,
    DROP COLUMN IF EXISTS cache_read_tokens,
    DROP COLUMN IF EXISTS cache_creation_tokens;

DROP TABLE IF EXISTS token_usage_log;

ALTER TABLE conversations
    DROP COLUMN IF EXISTS token_usage;
