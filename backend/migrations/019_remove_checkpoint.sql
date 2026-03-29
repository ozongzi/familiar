-- partial_checkpoint is no longer needed: the frontend uploads the partial
-- reply text directly with the interrupt request body (partial_reply field).
ALTER TABLE generation_jobs DROP COLUMN IF EXISTS partial_checkpoint;
