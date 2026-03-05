ALTER TABLE messages ADD COLUMN IF NOT EXISTS pinned BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX IF NOT EXISTS idx_messages_pinned ON messages (thread_id, pinned) WHERE pinned = true;
