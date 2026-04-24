-- Model picker access is now controlled by:
--   1. models.initial_available
--   2. user_model_permissions per-user overrides
--
-- Drop the older split flags so there is only one backend access model.
ALTER TABLE models
    DROP COLUMN IF EXISTS visible,
    DROP COLUMN IF EXISTS admin_only;
