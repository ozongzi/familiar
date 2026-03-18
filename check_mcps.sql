-- Check migrated MCP data
SELECT 
    id,
    name,
    type,
    jsonb_pretty(config) as config_formatted
FROM global_mcps
ORDER BY created_at;
