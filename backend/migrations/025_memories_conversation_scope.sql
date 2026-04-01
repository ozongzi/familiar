ALTER TABLE user_memories
    ADD COLUMN conversation_id UUID REFERENCES conversations(id) ON DELETE CASCADE;

DROP INDEX idx_user_memories_user;
CREATE INDEX idx_user_memories_user       ON user_memories (user_id, id) WHERE conversation_id IS NULL;
CREATE INDEX idx_user_memories_conv       ON user_memories (conversation_id, id) WHERE conversation_id IS NOT NULL;
