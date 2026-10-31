-- migrate-055: labels. A reader's own names for conversations; putting one
-- on a conversation moves nothing. Mirrored in init-schema.sql.
CREATE TABLE IF NOT EXISTS labels (
    account_address TEXT NOT NULL,
    id              BIGINT NOT NULL,
    name            TEXT NOT NULL,
    color           TEXT NOT NULL,
    created_at      BIGINT NOT NULL,
    PRIMARY KEY (account_address, id)
);
CREATE TABLE IF NOT EXISTS thread_labels (
    account_address TEXT NOT NULL,
    label_id        BIGINT NOT NULL,
    thread_id       TEXT NOT NULL,
    PRIMARY KEY (account_address, label_id, thread_id)
);
CREATE INDEX IF NOT EXISTS idx_thread_labels_thread ON thread_labels(account_address, thread_id);
