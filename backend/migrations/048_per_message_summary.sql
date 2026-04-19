-- Per-message summary anchor.
--
-- Moves the compact summary off `conversations` (where it was a single
-- per-conversation blob) onto the individual message it's anchored on.
-- Semantics: when `messages.summary_text` is non-NULL, it means "the
-- conversation path from the root up to and including this message is
-- already concisely captured by this summary".  Any branch that traverses
-- this message in its ancestor chain can reuse the summary for free, and
-- branches that don't traverse it correctly fall back to raw history.
--
-- `summary_tokens` is the estimated token count of `summary_text`, used
-- by the raw-first loading logic in compact.rs to decide whether the
-- summary fits the budget when raw doesn't.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS summary_text   TEXT,
    ADD COLUMN IF NOT EXISTS summary_tokens INTEGER;

-- Migrate any existing per-conversation summary onto its anchor message.
-- `compact_until_msg_id` was the "last summarised message" in the old
-- model, which matches our new anchor semantics exactly.
UPDATE messages m
SET summary_text = c.compact_summary
FROM conversations c
WHERE m.id = c.compact_until_msg_id
  AND c.compact_summary IS NOT NULL;

ALTER TABLE conversations
    DROP COLUMN IF EXISTS compact_summary,
    DROP COLUMN IF EXISTS compact_until_msg_id,
    DROP COLUMN IF EXISTS compact_at;
