ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS token_usage JSONB NOT NULL DEFAULT '{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}';
