-- Per-model visibility for the user-facing model picker.
-- Models marked as cheap/embedding should not show up in the picker by default;
-- admins can still toggle visibility per model.
ALTER TABLE models ADD COLUMN IF NOT EXISTS visible BOOLEAN NOT NULL DEFAULT true;

UPDATE models SET visible = false WHERE role IS NOT NULL;
