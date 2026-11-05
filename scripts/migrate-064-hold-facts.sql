-- migrate-064: every hold of a conversation, as a fact. Appended, never
-- updated: when, on what (`ingest` or `report`), under which rule pack, and
-- which of the reader's messages were unread at that moment (a JSON array
-- of Message-IDs). A hold no longer marks anything read. Mirrored in
-- init-schema.sql.
CREATE TABLE IF NOT EXISTS hold_facts (
    id              BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    held_at         BIGINT NOT NULL,
    source          TEXT NOT NULL,
    rules_version   TEXT NOT NULL DEFAULT '',
    unread_mids     JSONB NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS hold_facts_thread
    ON hold_facts (account_address, thread_id);
