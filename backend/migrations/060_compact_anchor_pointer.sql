-- Replace the "summary as text on anchor message" scheme with a pointer:
-- the anchor message (the assistant's final natural reply after a system
-- checkpoint inject) carries `summary_start_id`, pointing at the oldest
-- message in the active branch that should still be sent to the model
-- alongside the anchor. Loading walks from active tip backward; the first
-- message with a non-NULL summary_start_id is the anchor, and we keep
-- everything from `summary_start_id .. tip` (inclusive).
--
-- The old summary_text/summary_tokens columns are dropped — the anchor's
-- own `content` is the summary now, fully visible in the conversation.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS summary_start_id BIGINT
        REFERENCES messages(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_messages_summary_start
    ON messages (summary_start_id)
    WHERE summary_start_id IS NOT NULL;

ALTER TABLE messages
    DROP COLUMN IF EXISTS summary_text,
    DROP COLUMN IF EXISTS summary_tokens;
