CREATE TABLE mcp_catalog (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    description TEXT        NOT NULL DEFAULT '',
    command     TEXT        NOT NULL,
    args        JSONB       NOT NULL DEFAULT '[]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Migrate existing JSONB array from app_config
INSERT INTO mcp_catalog (name, description, command, args)
SELECT
    entry->>'name',
    COALESCE(entry->>'description', ''),
    COALESCE(entry->>'command', ''),
    COALESCE(entry->'args', '[]'::jsonb)
FROM app_config,
     jsonb_array_elements(COALESCE(mcp_catalog, '[]'::jsonb)) AS entry
WHERE mcp_catalog IS NOT NULL
  AND jsonb_array_length(mcp_catalog) > 0
  AND (entry->>'name') IS NOT NULL
  AND (entry->>'command') IS NOT NULL;

ALTER TABLE app_config DROP COLUMN IF EXISTS mcp_catalog;
