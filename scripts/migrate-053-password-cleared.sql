-- migrate-053: an administrator cleared the account's password, until a new
-- one is set: when, and who. Mirrored in init-schema.sql.
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS password_cleared_at BIGINT;
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS password_cleared_by TEXT;
