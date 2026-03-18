-- Diagnosis: Check current MCP configurations
SELECT 
    name,
    type,
    config->>'command' as command,
    config->>'args' as args_json,
    config->>'env' as env_json,
    config->>'url' as url
FROM global_mcps
ORDER BY name;

-- Fix: If MCP entries are corrupted, you can manually recreate them
-- Example for the "search" MCP with Tavily:

/*
DELETE FROM global_mcps WHERE name = 'search';

INSERT INTO global_mcps (name, type, config)
VALUES (
    'search',
    'stdio',
    jsonb_build_object(
        'command', 'npx',
        'args', jsonb_build_array('-y', 'tavily-mcp'),
        'env', jsonb_build_object('TAVILY_API_KEY', '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')
    )
);
*/

-- The key difference: 
-- Instead of: sh -c "TAVILY_API_KEY=xxx npx -y tavily-mcp"
-- Use proper env field: 
--   command: npx
--   args: ["-y", "tavily-mcp"]
--   env: {"TAVILY_API_KEY": "xxx"}
