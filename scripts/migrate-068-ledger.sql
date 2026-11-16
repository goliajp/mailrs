-- migrate-068: the fact ledger, as the kevy store keeps it: what each
-- delivered message was for its reader (written once, never edited), a
-- row per link host worth finding mail by (kept until `until`) and per
-- attachment hash, and the verdict the rules last gave (derived, replaced
-- at will). Re-judging after a reputation change finds mail through it.
-- Lists are space-separated, as in the kevy store. Mirrored in
-- init-schema.sql.
CREATE TABLE IF NOT EXISTS ledger_messages (
    user_address TEXT NOT NULL,
    message_id TEXT NOT NULL,
    arrived BIGINT NOT NULL,
    from_addr TEXT NOT NULL,
    from_domain TEXT NOT NULL,
    from_reg TEXT NOT NULL,
    reply_to TEXT NOT NULL,
    auth TEXT NOT NULL,
    link_hosts TEXT NOT NULL,
    attachment_sha256 TEXT NOT NULL,
    PRIMARY KEY (user_address, message_id)
);
CREATE INDEX IF NOT EXISTS idx_ledger_messages_reg ON ledger_messages (from_reg);
CREATE TABLE IF NOT EXISTS ledger_links (
    user_address TEXT NOT NULL,
    message_id TEXT NOT NULL,
    host TEXT NOT NULL,
    reg TEXT NOT NULL,
    until BIGINT,
    PRIMARY KEY (user_address, message_id, host)
);
CREATE INDEX IF NOT EXISTS idx_ledger_links_reg ON ledger_links (reg);
CREATE INDEX IF NOT EXISTS idx_ledger_links_until ON ledger_links (until) WHERE until IS NOT NULL;
CREATE TABLE IF NOT EXISTS ledger_files (
    user_address TEXT NOT NULL,
    message_id TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    PRIMARY KEY (user_address, message_id, sha256)
);
CREATE INDEX IF NOT EXISTS idx_ledger_files_sha256 ON ledger_files (sha256);
CREATE TABLE IF NOT EXISTS ledger_verdicts (
    user_address TEXT NOT NULL,
    message_id TEXT NOT NULL,
    rules TEXT NOT NULL,
    score DOUBLE PRECISION NOT NULL,
    rules_version TEXT NOT NULL,
    judged_at BIGINT NOT NULL,
    holds BOOLEAN NOT NULL,
    screening TEXT NOT NULL,
    PRIMARY KEY (user_address, message_id)
);
