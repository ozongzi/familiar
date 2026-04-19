-- Per-assistant-message token counts (from provider usage reporting).
-- NULL for user / tool_result messages; populated for assistant messages.
ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS prompt_tokens         BIGINT,
    ADD COLUMN IF NOT EXISTS completion_tokens     BIGINT,
    ADD COLUMN IF NOT EXISTS cache_read_tokens     BIGINT,
    ADD COLUMN IF NOT EXISTS cache_creation_tokens BIGINT;

-- Boundary for compact summary: the summary in `compact_summary` covers all
-- messages with id <= compact_until_msg_id. Messages with id > this value are
-- the "recent tail" kept raw. NULL means no compaction has run yet.
ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS compact_until_msg_id BIGINT;
