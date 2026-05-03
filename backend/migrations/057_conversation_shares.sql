-- Public share links for conversations. One token per conversation.
-- Anyone with the token can view a read-only snapshot of the active branch.

CREATE TABLE conversation_shares (
    token           TEXT        PRIMARY KEY,
    conversation_id UUID        NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    created_by      UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_conversation_shares_conv ON conversation_shares(conversation_id);
