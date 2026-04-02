-- Drop is_summary column from messages.
-- Summary is now stored exclusively in conversations.compact_summary.
-- restore() no longer needs a cutoff — it returns all real messages,
-- and the worker prepends the summary as the first user message.
ALTER TABLE messages DROP COLUMN IF EXISTS is_summary;
