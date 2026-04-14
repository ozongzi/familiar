ALTER TABLE app_config
    ADD COLUMN IF NOT EXISTS siliconflow_api_key TEXT,
    DROP COLUMN IF EXISTS fal_api_key,
    DROP COLUMN IF EXISTS cephalon_hmac_key,
    DROP COLUMN IF EXISTS cephalon_hmac_secret;
