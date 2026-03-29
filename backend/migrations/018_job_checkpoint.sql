-- Track the last generation_events.id that has been committed into a persisted
-- assistant message by the worker.  The interrupt handler reads tokens with
-- id > partial_checkpoint to reconstruct the in-flight partial reply so it can
-- save it before spawning the replacement job.
ALTER TABLE generation_jobs
    ADD COLUMN partial_checkpoint BIGINT NOT NULL DEFAULT 0;
