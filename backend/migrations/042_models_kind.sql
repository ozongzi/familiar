-- Distinguish API-driven models ('api', default) from CLI-driven backends
-- ('claude-code') that ride a subscription OAuth session via a child process
-- instead of calling an HTTP API. For 'claude-code' rows, model_name is the
-- `claude --model` flag (sonnet | opus | haiku | full model id); api_base /
-- api_key / provider are ignored.
ALTER TABLE models
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'api'
        CHECK (kind IN ('api', 'claude-code'));
