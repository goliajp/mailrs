-- mailrs PG schema
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE domains (
    name TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE accounts (
    address TEXT PRIMARY KEY,
    domain TEXT NOT NULL REFERENCES domains(name) ON DELETE CASCADE,
    display_name TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT true,
    quota_bytes BIGINT NOT NULL DEFAULT 1073741824,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE aliases (
    id BIGSERIAL PRIMARY KEY,
    source_address TEXT NOT NULL,
    target_address TEXT NOT NULL,
    domain TEXT NOT NULL REFERENCES domains(name) ON DELETE CASCADE,
    alias_type TEXT NOT NULL DEFAULT 'alias',
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(source_address, target_address)
);
CREATE INDEX idx_aliases_source ON aliases(source_address) WHERE active = true;

CREATE TABLE sieve_scripts (
    address TEXT PRIMARY KEY REFERENCES accounts(address) ON DELETE CASCADE,
    script TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE mailboxes (
    id BIGSERIAL PRIMARY KEY,
    user_address TEXT NOT NULL,
    name TEXT NOT NULL,
    uidvalidity INTEGER NOT NULL,
    uidnext INTEGER NOT NULL DEFAULT 1,
    highest_modseq BIGINT NOT NULL DEFAULT 0,
    UNIQUE(user_address, name)
);

CREATE TABLE messages (
    id BIGSERIAL PRIMARY KEY,
    mailbox_id BIGINT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    uid INTEGER NOT NULL,
    maildir_id TEXT NOT NULL,
    sender TEXT NOT NULL DEFAULT '',
    recipients TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT '',
    date_epoch BIGINT NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    flags INTEGER NOT NULL DEFAULT 0,
    internal_date BIGINT NOT NULL,
    message_id TEXT NOT NULL DEFAULT '',
    in_reply_to TEXT NOT NULL DEFAULT '',
    thread_id TEXT NOT NULL DEFAULT '',
    modseq BIGINT NOT NULL DEFAULT 0,
    UNIQUE(mailbox_id, uid)
);
CREATE INDEX idx_messages_date ON messages(mailbox_id, date_epoch DESC);
CREATE INDEX idx_messages_thread ON messages(thread_id);
CREATE INDEX idx_messages_message_id ON messages(message_id);
CREATE INDEX idx_messages_modseq ON messages(mailbox_id, modseq);

CREATE TABLE outbound_queue (
    id BIGSERIAL PRIMARY KEY,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    domain TEXT NOT NULL,
    message_data BYTEA NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 8,
    next_retry BIGINT NOT NULL,
    last_error TEXT,
    message_id TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    is_forwarded BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX idx_queue_pending ON outbound_queue(status, next_retry)
    WHERE status = 'pending';

CREATE TABLE dmarc_results (
    id BIGSERIAL PRIMARY KEY,
    report_date DATE NOT NULL DEFAULT CURRENT_DATE,
    source_ip TEXT NOT NULL,
    from_domain TEXT NOT NULL,
    spf_result TEXT NOT NULL,
    dkim_result TEXT NOT NULL,
    dmarc_result TEXT NOT NULL,
    disposition TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_dmarc_date ON dmarc_results(report_date);

CREATE TABLE greylist_triplets (
    key TEXT PRIMARY KEY,
    first_seen BIGINT NOT NULL,
    last_seen BIGINT NOT NULL
);
