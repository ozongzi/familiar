-- Persist token usage independently from conversations so deleting a
-- conversation does not erase its token statistics.
CREATE TABLE IF NOT EXISTS token_usage_log (
    id               BIGSERIAL PRIMARY KEY,
    conversation_id  UUID        NOT NULL,
    user_id          UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conversation_name TEXT,
    prompt_tokens    BIGINT      NOT NULL DEFAULT 0,
    completion_tokens BIGINT     NOT NULL DEFAULT 0,
    total_tokens     BIGINT      NOT NULL DEFAULT 0,
    recorded_at      BIGINT      NOT NULL  -- unix timestamp
);

CREATE INDEX IF NOT EXISTS token_usage_log_user_id    ON token_usage_log(user_id);
CREATE INDEX IF NOT EXISTS token_usage_log_conv_id    ON token_usage_log(conversation_id);
CREATE UNIQUE INDEX IF NOT EXISTS token_usage_log_conv_unique ON token_usage_log(conversation_id);

-- Backfill from existing conversations
INSERT INTO token_usage_log (conversation_id, user_id, conversation_name,
                              prompt_tokens, completion_tokens, total_tokens, recorded_at)
SELECT c.id,
       c.user_id,
       c.name,
       COALESCE((c.token_usage->>'prompt_tokens')::bigint, 0),
       COALESCE((c.token_usage->>'completion_tokens')::bigint, 0),
       COALESCE((c.token_usage->>'total_tokens')::bigint, 0),
       EXTRACT(EPOCH FROM NOW())::bigint
FROM conversations c
WHERE COALESCE((c.token_usage->>'total_tokens')::bigint, 0) > 0
ON CONFLICT (conversation_id) DO NOTHING;
