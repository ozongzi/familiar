ALTER TABLE conversations
    ADD COLUMN compact_summary TEXT,
    ADD COLUMN compact_at TIMESTAMPTZ;
