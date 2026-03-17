CREATE TABLE app_config (
    id          BOOLEAN PRIMARY KEY DEFAULT true,
    config_json JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
