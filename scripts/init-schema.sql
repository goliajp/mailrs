-- mailrs PG/SPG schema (dual-target since Phase D-pre #4)
-- `CREATE EXTENSION vector` lives in scripts/pg-extensions.sql, mounted as
-- /docker-entrypoint-initdb.d/00-pg-extensions.sql for the PG image only.
-- SPG ships VECTOR(N) builtin (no extension system) and rejects CREATE EXTENSION.

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
    pinned BOOLEAN NOT NULL DEFAULT false,
    archived BOOLEAN NOT NULL DEFAULT false,
    text_body TEXT,
    html_body TEXT,
    clean_text TEXT,
    new_content TEXT,
    importance_level TEXT NOT NULL DEFAULT 'normal',
    importance_score REAL NOT NULL DEFAULT 0.0,
    is_bulk_sender BOOLEAN NOT NULL DEFAULT false,
    has_tracking_pixel BOOLEAN NOT NULL DEFAULT false,
    UNIQUE(mailbox_id, uid)
);
CREATE INDEX idx_messages_date ON messages(mailbox_id, date_epoch DESC);
CREATE INDEX idx_messages_thread ON messages(thread_id);
CREATE INDEX idx_messages_message_id ON messages(message_id);
CREATE INDEX idx_messages_modseq ON messages(mailbox_id, modseq);
CREATE INDEX idx_messages_importance ON messages(mailbox_id, importance_level, internal_date DESC);

CREATE TABLE outbound_queue (
    id BIGSERIAL PRIMARY KEY,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    domain TEXT NOT NULL,
    message_data TEXT NOT NULL,  -- base64-encoded payload (Phase D-pre #3)
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
    triplet TEXT PRIMARY KEY,
    first_seen BIGINT NOT NULL,
    last_seen BIGINT NOT NULL
);

CREATE TABLE contacts (
    id              BIGSERIAL PRIMARY KEY,
    user_address    TEXT NOT NULL,
    email           TEXT NOT NULL,
    display_name    TEXT NOT NULL DEFAULT '',
    first_seen      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen       TIMESTAMPTZ NOT NULL DEFAULT now(),
    received_count  INT NOT NULL DEFAULT 0,
    sent_count      INT NOT NULL DEFAULT 0,
    reply_count     INT NOT NULL DEFAULT 0,
    is_mutual       BOOLEAN NOT NULL DEFAULT false,
    is_mailing_list BOOLEAN NOT NULL DEFAULT false,
    is_automated    BOOLEAN NOT NULL DEFAULT false,
    organization    TEXT NOT NULL DEFAULT '',
    title           TEXT NOT NULL DEFAULT '',
    phone           TEXT NOT NULL DEFAULT '',
    importance_bias REAL NOT NULL DEFAULT 0.0,
    is_vip          BOOLEAN NOT NULL DEFAULT false,
    is_blocked      BOOLEAN NOT NULL DEFAULT false,
    relationship_score REAL NOT NULL DEFAULT 0.0,
    UNIQUE(user_address, email)
);
CREATE INDEX idx_contacts_user_score ON contacts(user_address, relationship_score DESC);
CREATE INDEX idx_contacts_user_email ON contacts(user_address, email);

CREATE TABLE sender_feedback (
    id              BIGSERIAL PRIMARY KEY,
    user_address    TEXT NOT NULL,
    sender_email    TEXT NOT NULL,
    action          TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_sender_feedback_user ON sender_feedback(user_address, sender_email);

CREATE TABLE email_analysis (
    message_id BIGINT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    category TEXT NOT NULL DEFAULT 'general',
    risk_score SMALLINT NOT NULL DEFAULT 0,
    risk_reason TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    people JSONB NOT NULL DEFAULT '[]',
    dates JSONB NOT NULL DEFAULT '[]',
    amounts JSONB NOT NULL DEFAULT '[]',
    action_items JSONB NOT NULL DEFAULT '[]',
    embedding vector(1024),
    model_version TEXT NOT NULL DEFAULT '',
    analyzed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_ea_category ON email_analysis(category);
CREATE INDEX idx_ea_embedding ON email_analysis
    USING hnsw (embedding vector_cosine_ops);

CREATE TABLE IF NOT EXISTS reactions (
    id BIGSERIAL PRIMARY KEY,
    message_uid BIGINT NOT NULL,
    thread_id TEXT NOT NULL,
    account_address TEXT NOT NULL,
    emoji TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(message_uid, account_address, emoji)
);
CREATE INDEX IF NOT EXISTS idx_reactions_thread ON reactions(thread_id);

CREATE TABLE IF NOT EXISTS snoozed_conversations (
    thread_id TEXT NOT NULL,
    account_address TEXT NOT NULL,
    snoozed_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (thread_id, account_address)
);
CREATE INDEX IF NOT EXISTS idx_snoozed_until ON snoozed_conversations(snoozed_until);

CREATE TABLE IF NOT EXISTS webhook_subscriptions (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL,
    url TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'new_message',
    filter_sender TEXT,
    filter_thread_id TEXT,
    signing_secret TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_webhook_subs_account ON webhook_subscriptions(account_address) WHERE active = true;
CREATE INDEX IF NOT EXISTS idx_webhook_subs_event ON webhook_subscriptions(event_type, active) WHERE active = true;

CREATE TABLE IF NOT EXISTS webhook_outbox (
    id BIGSERIAL PRIMARY KEY,
    subscription_id BIGINT NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 8,
    next_retry BIGINT NOT NULL,
    last_error TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_webhook_outbox_pending ON webhook_outbox(status, next_retry)
    WHERE status = 'pending';

-- system config: runtime-editable configuration
CREATE TABLE IF NOT EXISTS system_config (
    config_key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    value_type TEXT NOT NULL DEFAULT 'string',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL DEFAULT ''
);

-- vacation auto-reply dedup state (RFC 5230 §4.6) — see migrate-037
CREATE TABLE IF NOT EXISTS vacation_dedup (
    recipient    TEXT NOT NULL,
    sender       TEXT NOT NULL,
    handle       TEXT NOT NULL,
    last_sent_at BIGINT NOT NULL,
    PRIMARY KEY (recipient, sender, handle)
);
