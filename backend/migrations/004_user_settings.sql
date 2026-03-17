CREATE TABLE user_settings (
    user_id          UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    frontier_model   JSONB, -- { "name": "...", "api_key": "...", "api_base": "...", "extra_body": {} }
    cheap_model      JSONB,
    system_prompt    TEXT,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
