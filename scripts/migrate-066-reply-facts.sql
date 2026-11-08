-- migrate-066: two more facts delivery records for the sectioned inbox:
-- whether the message is a calendar invitation, and whether the reader and
-- its sender write to each other. NULL is mail recorded before them, read
-- as false, which keeps it out of Reply unless the reader wrote in the
-- conversation. Mirrored in init-schema.sql.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS invite BOOLEAN;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS mutual BOOLEAN;
