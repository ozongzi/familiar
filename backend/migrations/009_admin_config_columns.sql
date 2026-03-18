-- ── App Config Refactor ──────────────────────────────────────────────────────

-- Add individual columns to app_config
ALTER TABLE app_config ADD COLUMN public_path TEXT;
ALTER TABLE app_config ADD COLUMN artifacts_path TEXT;
ALTER TABLE app_config ADD COLUMN frontier_model JSONB;
ALTER TABLE app_config ADD COLUMN cheap_model JSONB;
ALTER TABLE app_config ADD COLUMN embedding_model JSONB;
ALTER TABLE app_config ADD COLUMN server_port INT;
ALTER TABLE app_config ADD COLUMN system_prompt TEXT;
ALTER TABLE app_config ADD COLUMN subagent_prompt TEXT;
ALTER TABLE app_config ADD COLUMN mcp_catalog JSONB;

-- Migrate data from config_json (if exists)
UPDATE app_config SET
    public_path = config_json->>'public_path',
    artifacts_path = config_json->>'artifacts_path',
    frontier_model = config_json->'frontier_model',
    cheap_model = config_json->'cheap_model',
    embedding_model = config_json->'embedding_model',
    mcp_catalog = config_json->'mcp_catalog',
    server_port = (config_json->>'port')::INT,  -- Assuming nested under 'server' -> 'port'? No, config structure was: { "server": { "port": ... } } ?
    -- Let's check config.rs structure to be sure.
    -- Assuming server config was top-level or nested. Based on AdminConfig.tsx: server: { port, system_prompt, subagent_prompt }
    -- So system_prompt = config_json->'server'->>'system_prompt'
    system_prompt = config_json->'server'->>'system_prompt',
    subagent_prompt = config_json->'server'->>'subagent_prompt'
WHERE config_json IS NOT NULL;

-- Fix server_port extraction (nested)
UPDATE app_config SET
    server_port = (config_json->'server'->>'port')::INT
WHERE config_json IS NOT NULL AND server_port IS NULL;

-- Drop old column moved to end
-- ALTER TABLE app_config DROP COLUMN config_json;


-- ── Global MCPs ──────────────────────────────────────────────────────────────

CREATE TABLE global_mcps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    type TEXT NOT NULL CHECK (type IN ('http', 'stdio')),
    config JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Migrate existing MCPs from config_json
INSERT INTO global_mcps (name, type, config)
SELECT 
    elem->>'name',
    CASE WHEN (elem->>'url') IS NOT NULL THEN 'http' ELSE 'stdio' END,
    elem - 'name'
FROM app_config, jsonb_array_elements(COALESCE(config_json->'mcp', '[]'::jsonb)) AS elem
ON CONFLICT (name) DO NOTHING;

-- Drop old column
ALTER TABLE app_config DROP COLUMN config_json;
