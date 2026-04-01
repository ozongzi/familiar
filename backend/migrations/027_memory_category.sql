ALTER TABLE user_memories
    ADD COLUMN category TEXT NOT NULL DEFAULT 'note'
        CHECK (category IN ('preference', 'procedure', 'fact', 'note')),
    ADD COLUMN embedding vector(1536);

CREATE INDEX idx_user_memories_embedding
    ON user_memories USING ivfflat (embedding vector_cosine_ops)
    WHERE embedding IS NOT NULL;
