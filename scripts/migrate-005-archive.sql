ALTER TABLE messages ADD COLUMN IF NOT EXISTS archived BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX IF NOT EXISTS idx_messages_archived ON messages (thread_id, archived) WHERE archived = true;
