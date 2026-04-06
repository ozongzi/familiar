CREATE TABLE invite_codes (
    code       TEXT        PRIMARY KEY,
    created_by UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    used_by    UUID        REFERENCES users(id) ON DELETE SET NULL,
    used_at    TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
