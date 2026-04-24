-- Per-user visibility overrides for global models.
--
-- A missing row means "inherit models.initial_available". An explicit row
-- allows admins to grant or deny a global model to a user.
CREATE TABLE user_model_permissions (
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    model_id   UUID        NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    allowed    BOOLEAN     NOT NULL,
    updated_by UUID        REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, model_id)
);

CREATE INDEX user_model_permissions_model_idx ON user_model_permissions (model_id);
