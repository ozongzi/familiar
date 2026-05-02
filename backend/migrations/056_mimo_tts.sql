ALTER TABLE app_config
    ADD COLUMN IF NOT EXISTS mimo_tts_api_key TEXT,
    ADD COLUMN IF NOT EXISTS mimo_tts_api_base TEXT NOT NULL DEFAULT 'https://api.xiaomimimo.com/v1',
    ADD COLUMN IF NOT EXISTS mimo_tts_model TEXT NOT NULL DEFAULT 'mimo-v2.5-tts',
    ADD COLUMN IF NOT EXISTS mimo_tts_voice TEXT NOT NULL DEFAULT 'mimo_default',
    ADD COLUMN IF NOT EXISTS mimo_tts_style TEXT NOT NULL DEFAULT '(河南话)';

CREATE TABLE IF NOT EXISTS message_tts_cache (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id   BIGINT      NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    model        TEXT        NOT NULL,
    voice        TEXT        NOT NULL,
    style        TEXT        NOT NULL,
    format       TEXT        NOT NULL,
    content_hash TEXT        NOT NULL,
    file_path    TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (message_id, model, voice, style, format, content_hash)
);

CREATE INDEX IF NOT EXISTS message_tts_cache_message_idx ON message_tts_cache(message_id);
