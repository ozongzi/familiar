-- Per-model admin-only flag.
-- Some models (e.g. Claude Code riding the Max plan OAuth) must stay restricted
-- to the instance operator to comply with the provider's ToS. When set, only
-- users with is_admin=true see the model in their picker and can dispatch it.
ALTER TABLE models ADD COLUMN IF NOT EXISTS admin_only BOOLEAN NOT NULL DEFAULT false;
