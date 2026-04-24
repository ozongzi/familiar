-- Default availability for global models before per-user overrides are applied.
--
-- This replaces the old split between `visible` and `admin_only` for model
-- picker access. Existing data is folded into the new default:
-- visible non-admin-only models start available; hidden or admin-only models
-- start unavailable until the permission matrix grants them.
ALTER TABLE models
    ADD COLUMN IF NOT EXISTS initial_available BOOLEAN NOT NULL DEFAULT true;

UPDATE models
SET initial_available = COALESCE(visible, true) AND NOT COALESCE(admin_only, false)
WHERE scope = 'global';
