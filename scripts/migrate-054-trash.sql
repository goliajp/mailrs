-- migrate-054: the Trash. A conversation a reader deleted is kept, out of
-- every other list, until restored, deleted for good, or 30 days pass.
-- Mirrored in init-schema.sql.
CREATE TABLE IF NOT EXISTS trashed_conversations (
    account_address TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    trashed_at      BIGINT NOT NULL,
    PRIMARY KEY (account_address, thread_id)
);
CREATE INDEX IF NOT EXISTS idx_trashed_at ON trashed_conversations(trashed_at);
