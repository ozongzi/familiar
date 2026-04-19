-- Per-model compaction thresholds. NOT NULL + DEFAULT reproduces the previous
-- hardcoded constants so existing rows keep the current behaviour.
--
-- compact_trigger_tokens: context size (prompt_tokens) that triggers a compact
-- compact_tail_tokens:    tokens of recent history kept raw after a compact
--
-- The truncate-safety ceiling used by the worker is derived from trigger
-- (trigger * 5/4), so this migration is enough — no history_budget column.
ALTER TABLE models
    ADD COLUMN compact_trigger_tokens BIGINT NOT NULL DEFAULT 50000,
    ADD COLUMN compact_tail_tokens    BIGINT NOT NULL DEFAULT 16000;
