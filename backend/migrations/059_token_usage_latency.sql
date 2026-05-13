ALTER TABLE token_usage_events
    ADD COLUMN IF NOT EXISTS ttft_ms  BIGINT,
    ADD COLUMN IF NOT EXISTS total_ms BIGINT;

CREATE INDEX IF NOT EXISTS idx_token_usage_events_ttft
    ON token_usage_events (ttft_ms)
    WHERE ttft_ms IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_token_usage_events_total
    ON token_usage_events (total_ms)
    WHERE total_ms IS NOT NULL;
