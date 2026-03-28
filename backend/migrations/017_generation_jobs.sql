-- Generation jobs: one row per user-triggered generation (may span multiple tool-call rounds).
CREATE TABLE generation_jobs (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID        NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id         UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status          TEXT        NOT NULL DEFAULT 'pending',   -- pending | running | done | aborted | error
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_gen_jobs_conv ON generation_jobs(conversation_id, created_at DESC);
CREATE INDEX idx_gen_jobs_status ON generation_jobs(status) WHERE status IN ('pending', 'running');

-- Generation events: every SSE-worthy event a worker produces.
-- The SSE handler reads from here (replay + live via LISTEN/NOTIFY).
CREATE TABLE generation_events (
    id         BIGSERIAL   PRIMARY KEY,
    job_id     UUID        NOT NULL REFERENCES generation_jobs(id) ON DELETE CASCADE,
    payload    TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_gen_events_job ON generation_events(job_id, id);

-- Trigger: notify listeners when a new event is inserted.
CREATE OR REPLACE FUNCTION notify_generation_event() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('generation_events', NEW.job_id::text || ':' || NEW.id::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_generation_event
    AFTER INSERT ON generation_events
    FOR EACH ROW EXECUTE FUNCTION notify_generation_event();

-- Trigger: notify when job status changes (for abort detection).
CREATE OR REPLACE FUNCTION notify_generation_job_status() RETURNS trigger AS $$
BEGIN
    IF OLD.status IS DISTINCT FROM NEW.status THEN
        PERFORM pg_notify('generation_job_status', NEW.id::text || ':' || NEW.status);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_generation_job_status
    AFTER UPDATE ON generation_jobs
    FOR EACH ROW EXECUTE FUNCTION notify_generation_job_status();
