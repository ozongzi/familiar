-- Cross-provider thinking-mode dial. NULL = leave provider default in place
-- (DeepSeek defaults to thinking on; Anthropic 4.6+ defaults to thinking off).
-- 'none' explicitly disables thinking on providers that support a toggle.
ALTER TABLE models
    ADD COLUMN IF NOT EXISTS reasoning_effort TEXT
    CHECK (reasoning_effort IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'));
