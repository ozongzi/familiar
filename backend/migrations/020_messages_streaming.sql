-- Add streaming state to messages table.
-- A row with streaming=true is an in-flight assistant message being written
-- token-by-token by the worker.  It acts as the single source of truth for
-- the current partial reply, replacing the need to reconstruct content from
-- generation_events on reconnect.
ALTER TABLE messages
    ADD COLUMN streaming BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN job_id    UUID    REFERENCES generation_jobs(id) ON DELETE SET NULL;

-- Fast lookup: "find the live streaming row for this job"
CREATE INDEX idx_messages_streaming ON messages (job_id) WHERE streaming = true;
