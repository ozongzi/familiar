-- Add is_default flag to models table.
-- Only one global model should have is_default=true at a time (enforced in app logic).

ALTER TABLE models ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT false;

-- Migrate: if app_config has a frontier_model, insert it as a default global model.
-- This is best-effort; will only run if the column exists and has data.
INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, is_default)
SELECT
    NULL,
    'global',
    COALESCE(frontier_model->>'name', 'Default'),
    COALESCE(frontier_model->>'provider', 'deepseek'),
    COALESCE(frontier_model->>'name', 'deepseek-chat'),
    COALESCE(frontier_model->>'api_base', ''),
    COALESCE(frontier_model->>'api_key', ''),
    true
FROM app_config
WHERE frontier_model IS NOT NULL
  AND frontier_model->>'name' IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM models WHERE is_default = true AND scope = 'global')
LIMIT 1;
