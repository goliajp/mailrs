-- migrate-059: team inboxes. Each member's copy of a team conversation,
-- with the team's assignee copied onto it (the assignment facts live in the
-- network kevy), and the reply claims that keep two members from both
-- answering. Mirrored in init-schema.sql.
CREATE TABLE IF NOT EXISTS shared_thread_copies (
    account_address TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    shared_conv     TEXT NOT NULL,
    assignee        TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (account_address, thread_id)
);
CREATE INDEX IF NOT EXISTS idx_shared_thread_copies_assignee
    ON shared_thread_copies(account_address, assignee);
CREATE TABLE IF NOT EXISTS shared_reply_state (
    shared_conv TEXT PRIMARY KEY,
    state       TEXT NOT NULL
);
