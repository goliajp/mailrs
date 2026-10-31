-- migrate-056: how many attachments each message carries, counted when it
-- is stored. NULL for mail stored before, which `has:attachment` does not
-- match. Mirrored in init-schema.sql.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS attachments INTEGER;
