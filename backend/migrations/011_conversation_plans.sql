CREATE TABLE conversation_plans (
    conversation_id UUID PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    title           TEXT        NOT NULL DEFAULT '',
    steps_json      TEXT        NOT NULL DEFAULT '[]',
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
