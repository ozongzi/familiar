-- Add role column to models: 'cheap' = cheap/fast model, 'embedding' = embedding model.
-- At most one global model should hold each role at a time (enforced in app logic).
ALTER TABLE models ADD COLUMN IF NOT EXISTS role TEXT CHECK (role IN ('cheap', 'embedding'));

-- Migrate cheap_model from app_config (best-effort, only if no cheap model exists yet)
INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, role)
SELECT
    NULL,
    'global',
    COALESCE(cheap_model->>'name', 'Cheap Model'),
    COALESCE(cheap_model->>'provider', 'deepseek'),
    COALESCE(cheap_model->>'name', 'deepseek-chat'),
    COALESCE(cheap_model->>'api_base', ''),
    COALESCE(cheap_model->>'api_key', ''),
    'cheap'
FROM app_config
WHERE cheap_model IS NOT NULL
  AND cheap_model->>'name' IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM models WHERE role = 'cheap' AND scope = 'global')
LIMIT 1;

-- Migrate embedding_model from app_config (best-effort, only if no embedding model exists yet)
INSERT INTO models (user_id, scope, label, provider, model_name, api_base, api_key, role)
SELECT
    NULL,
    'global',
    COALESCE(embedding_model->>'name', 'Embedding Model'),
    COALESCE(embedding_model->>'provider', 'openai'),
    COALESCE(embedding_model->>'name', 'text-embedding-3-small'),
    COALESCE(embedding_model->>'api_base', ''),
    COALESCE(embedding_model->>'api_key', ''),
    'embedding'
FROM app_config
WHERE embedding_model IS NOT NULL
  AND embedding_model->>'name' IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM models WHERE role = 'embedding' AND scope = 'global')
LIMIT 1;

-- Drop migrated and dead columns from app_config
ALTER TABLE app_config
    DROP COLUMN IF EXISTS cheap_model,
    DROP COLUMN IF EXISTS embedding_model,
    DROP COLUMN IF EXISTS public_path,
    DROP COLUMN IF EXISTS artifacts_path,
    DROP COLUMN IF EXISTS server_port,
    DROP COLUMN IF EXISTS subagent_prompt;
