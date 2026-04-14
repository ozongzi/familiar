ALTER TABLE token_usage_log
    ADD COLUMN IF NOT EXISTS cache_read_tokens      BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_creation_tokens  BIGINT NOT NULL DEFAULT 0;
