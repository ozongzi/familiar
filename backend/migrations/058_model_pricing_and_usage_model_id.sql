-- Per-token-type pricing on each model. USD per million tokens.
-- DOUBLE PRECISION is plenty for $/Mtok rates and avoids pulling a NUMERIC
-- crate into sqlx. NULL = price unknown; cost queries treat unknown as 0.
ALTER TABLE models
    ADD COLUMN IF NOT EXISTS price_input_per_mtoken          DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS price_output_per_mtoken         DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS price_cache_read_per_mtoken     DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS price_cache_creation_per_mtoken DOUBLE PRECISION;

-- Tag each token-usage row with the model that produced it, so cost queries
-- can JOIN models and apply the right per-type price. SET NULL on model
-- delete keeps history readable (price falls back to unknown / 0).
ALTER TABLE token_usage_events
    ADD COLUMN IF NOT EXISTS model_id UUID REFERENCES models(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_token_usage_events_model
    ON token_usage_events (model_id);

-- Backfill, in two passes:
--
-- (1) `conversations.model_id` was historically allowed to stay NULL — the
-- worker fell back to the global default at generation time. That left old
-- conversations with no permanent record of which model was used. The most
-- precise source is `generation_jobs.(provider, model)` (recorded per turn
-- since migration 040). Resolve back to a row in `models` and pin it onto
-- the conversation, preferring global rows when both global and user-scoped
-- variants of the same provider/model exist.
UPDATE conversations c
SET model_id = (
    SELECT m.id
    FROM generation_jobs g
    JOIN models m
      ON m.provider   = g.provider
     AND m.model_name = g.model
    WHERE g.conversation_id = c.id
      AND g.model IS NOT NULL
      AND g.provider IS NOT NULL
    ORDER BY g.created_at DESC,
             (m.scope = 'global') DESC,
             m.created_at ASC
    LIMIT 1
)
WHERE c.model_id IS NULL;

-- (2) Token-usage events get their model_id from the generation_job that
-- produced them (most accurate — captures per-turn model swaps too), with
-- conversation.model_id as a fallback for events whose job predates the
-- model/provider columns on generation_jobs.
UPDATE token_usage_events t
SET model_id = COALESCE(
    (
      SELECT m.id
      FROM generation_jobs g
      JOIN models m
        ON m.provider   = g.provider
       AND m.model_name = g.model
      WHERE g.id = t.job_id
        AND g.model IS NOT NULL
        AND g.provider IS NOT NULL
      ORDER BY (m.scope = 'global') DESC, m.created_at ASC
      LIMIT 1
    ),
    (SELECT c.model_id FROM conversations c WHERE c.id = t.conversation_id)
)
WHERE t.model_id IS NULL;

-- Drop the synthesised total — it was provider-agnostic sum of four columns
-- and conflated cache_read (10× cheaper, recurring) with new tokens. Going
-- forward we keep only what the provider returns and compute cost on demand.
ALTER TABLE token_usage_events
    DROP COLUMN IF EXISTS total_tokens;
