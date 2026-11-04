-- migrate-061: every release of a held conversation, as a fact. Appended,
-- never updated: who let it go, when, and on whose say (`person`, `batch`
-- or `rejudge`). Mirrored in init-schema.sql.
CREATE TABLE IF NOT EXISTS release_facts (
    id              BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    released_at     BIGINT NOT NULL,
    source          TEXT NOT NULL,
    actor           TEXT NOT NULL,
    rules_version   TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS release_facts_thread
    ON release_facts (account_address, thread_id);
