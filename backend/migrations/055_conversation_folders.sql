-- Conversation folders for organizing chats in a tree structure.

CREATE TABLE folders (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    parent_id  UUID REFERENCES folders(id) ON DELETE CASCADE,
    position   INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_folders_user   ON folders(user_id);
CREATE INDEX idx_folders_parent ON folders(parent_id);

ALTER TABLE conversations
    ADD COLUMN folder_id UUID REFERENCES folders(id) ON DELETE CASCADE;

CREATE INDEX idx_conversations_folder ON conversations(folder_id);
