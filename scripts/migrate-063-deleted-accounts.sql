-- migrate-063: a deleted account keeps its row, and with it every row that
-- references it, until its purge; `deleted_at` (unix seconds) marks it,
-- and only a purge deletes the row. A restore clears it. Mirrored in
-- init-schema.sql.
ALTER TABLE accounts ADD COLUMN IF NOT EXISTS deleted_at BIGINT;
