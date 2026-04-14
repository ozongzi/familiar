ALTER TABLE app_config
    ADD COLUMN IF NOT EXISTS cephalon_hmac_key TEXT,
    ADD COLUMN IF NOT EXISTS cephalon_hmac_secret TEXT;
