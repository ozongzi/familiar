-- Add latency columns to generation_jobs so TTFT and total duration
-- can be queried and aggregated without parsing log files.
ALTER TABLE generation_jobs
    ADD COLUMN IF NOT EXISTS ttft_ms       INTEGER,   -- ms from LLM stream open → first token
    ADD COLUMN IF NOT EXISTS duration_ms   INTEGER,   -- ms from job created_at → done/error
    ADD COLUMN IF NOT EXISTS model         TEXT,      -- model used for this generation
    ADD COLUMN IF NOT EXISTS provider      TEXT;      -- provider (anthropic, openai, …)
