-- Add parent_id to messages for branching support.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS parent_id BIGINT REFERENCES messages(id);
ALTER TABLE conversations ADD COLUMN IF NOT EXISTS active_message_id BIGINT REFERENCES messages(id);

WITH ordered AS (
    SELECT id, conversation_id,
           LAG(id) OVER (PARTITION BY conversation_id ORDER BY id) AS prev_id
    FROM messages
)
UPDATE messages m
SET parent_id = o.prev_id
FROM ordered o
WHERE m.id = o.id AND o.prev_id IS NOT NULL;

UPDATE conversations c
SET active_message_id = (
    SELECT MAX(id) FROM messages m WHERE m.conversation_id = c.id
);

CREATE INDEX IF NOT EXISTS idx_messages_parent_id ON messages(parent_id);
