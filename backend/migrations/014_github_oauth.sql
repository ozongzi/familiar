ALTER TABLE users
    ADD COLUMN IF NOT EXISTS github_id     TEXT        UNIQUE,
    ADD COLUMN IF NOT EXISTS display_name  TEXT,
    ADD COLUMN IF NOT EXISTS invite_code   TEXT        UNIQUE,
    ADD COLUMN IF NOT EXISTS last_login_at TIMESTAMPTZ;

-- password_hash is no longer required for GitHub-only users
ALTER TABLE users
    ALTER COLUMN password_hash DROP NOT NULL;
