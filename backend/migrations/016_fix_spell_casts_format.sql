-- Migrate spell_casts from OpenAI nested format to agentix flat format.
--
-- Old: [{"id":"…","type":"function","function":{"name":"…","arguments":"…"}}]
-- New: [{"id":"…","name":"…","arguments":"…"}]
--
-- Only rows where the first element has a "function" key need conversion.

UPDATE messages
SET spell_casts = (
    SELECT jsonb_agg(
        jsonb_build_object(
            'id',        elem->>'id',
            'name',      elem->'function'->>'name',
            'arguments', elem->'function'->>'arguments'
        )
    )::text
    FROM jsonb_array_elements(spell_casts::jsonb) AS elem
)
WHERE spell_casts IS NOT NULL
  AND spell_casts::jsonb -> 0 ? 'function';
