-- mailrs schema. Runs on PostgreSQL (+pgvector) and SPG alike —
-- SPG ships VECTOR(N) builtin and accepts CREATE EXTENSION as a no-op.

CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

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
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    recovery_email TEXT NOT NULL DEFAULT ''
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
    search_vector tsvector,
    bimi_logo_url TEXT,
    invite_payload JSONB,
    invite_method TEXT,
    rsvp_status TEXT,
    rsvp_at TIMESTAMPTZ,
    UNIQUE(mailbox_id, uid)
);
-- iTIP invite metadata lookups (migrate-032)
CREATE INDEX idx_messages_is_invite ON messages(mailbox_id)
    WHERE invite_payload IS NOT NULL;
CREATE INDEX idx_messages_invite_method ON messages(invite_method)
    WHERE invite_method IS NOT NULL;
CREATE INDEX idx_messages_date ON messages(mailbox_id, date_epoch DESC);
CREATE INDEX idx_messages_search_vector ON messages USING GIN (search_vector);

-- full-text search vector maintenance (mirrors migrate-016; the search
-- query in mailbox search_ops relies on this column existing)
CREATE OR REPLACE FUNCTION messages_search_vector_update() RETURNS trigger AS $$
BEGIN
  NEW.search_vector :=
    setweight(to_tsvector('simple', COALESCE(NEW.subject, '')), 'A') ||
    setweight(to_tsvector('simple', COALESCE(NEW.sender, '')), 'B') ||
    setweight(to_tsvector('simple', COALESCE(NEW.recipients, '')), 'B') ||
    setweight(to_tsvector('simple', COALESCE(NEW.clean_text, COALESCE(NEW.text_body, ''))), 'C');
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER messages_search_vector_trigger
  BEFORE INSERT OR UPDATE OF subject, sender, recipients, text_body, clean_text
  ON messages
  FOR EACH ROW
  EXECUTE FUNCTION messages_search_vector_update();
CREATE INDEX idx_messages_thread ON messages(thread_id);
CREATE INDEX idx_messages_message_id ON messages(message_id);
CREATE INDEX idx_messages_modseq ON messages(mailbox_id, modseq);
CREATE INDEX idx_messages_importance ON messages(mailbox_id, importance_level, internal_date DESC);

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

-- Phase 2 local white/black lists. UNIQUE (kind, value) without `list` is
-- the schema-level mutex: same key cannot be on both white and black at
-- the same time. To move an entry between lists, DELETE + INSERT (no
-- PATCH/PUT in the admin API by design).
CREATE TABLE greylist_local_lists (
    id          BIGSERIAL PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('domain', 'email', 'cidr')),
    list        TEXT NOT NULL CHECK (list IN ('white', 'black')),
    value       TEXT NOT NULL,
    note        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by  TEXT,
    UNIQUE (kind, value)
);
CREATE INDEX greylist_local_lists_kind_idx ON greylist_local_lists (kind);

-- email_contacts: per-user senders/recipients extracted from message traffic.
-- Distinct from the CardDAV "contacts" table created in migrate-023, which
-- holds vCard objects keyed by address_book_id. Keeping these two as separate
-- names avoids the `CREATE TABLE IF NOT EXISTS` silent-skip that hid migrate-023's
-- contacts behind this one for several releases.
CREATE TABLE email_contacts (
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
CREATE INDEX idx_email_contacts_user_score ON email_contacts(user_address, relationship_score DESC);
CREATE INDEX idx_email_contacts_user_email ON email_contacts(user_address, email);

CREATE TABLE sender_feedback (
    id              BIGSERIAL PRIMARY KEY,
    user_address    TEXT NOT NULL,
    sender_email    TEXT NOT NULL,
    action          TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_sender_feedback_user ON sender_feedback(user_address, sender_email);

-- attachment content extraction results (OCR, PDF text, etc.) —
-- search_conversations UNIONs over extracted_text (mirrors migrate-007)
CREATE TABLE attachment_content (
    id BIGSERIAL PRIMARY KEY,
    message_id BIGINT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    attachment_index SMALLINT NOT NULL,
    content_type TEXT NOT NULL,
    extracted_text TEXT,
    language TEXT,
    ocr_confidence DOUBLE PRECISION NOT NULL DEFAULT 0,
    page_count SMALLINT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(message_id, attachment_index)
);
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX idx_attachment_content_extracted_trgm
    ON attachment_content USING gin(extracted_text gin_trgm_ops)
    WHERE extracted_text IS NOT NULL AND extracted_text != '';
CREATE INDEX idx_attachment_content_message
    ON attachment_content(message_id) INCLUDE (attachment_index);

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
    analyzed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    clean_text TEXT NOT NULL DEFAULT '',
    requires_action BOOLEAN NOT NULL DEFAULT false,
    action_deadline TIMESTAMPTZ,
    sender_intent TEXT NOT NULL DEFAULT 'inform'
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
    key TEXT PRIMARY KEY,
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

-- ════════════════════════════════════════════════════════════════════
-- Objects below were historically created only by scripts/migrate-NNN
-- files, so a fresh init-schema boot was missing 27 tables + several
-- columns (discovered 2026-06-12: login 401'd on a fresh database
-- because accounts.recovery_email didn't exist). They are now part of
-- the canonical fresh schema. Equivalence gate: loading this file into
-- an empty database must pg_dump-match init + the full migration chain.
-- ════════════════════════════════════════════════════════════════════

-- saved email drafts (migrate-003)
CREATE TABLE drafts (
    id BIGSERIAL PRIMARY KEY,
    user_address TEXT NOT NULL,
    to_addresses TEXT NOT NULL DEFAULT '',
    cc_addresses TEXT NOT NULL DEFAULT '',
    bcc_addresses TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    reply_to_thread_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_drafts_user ON drafts(user_address, updated_at DESC);

-- user email templates (migrate-008)
CREATE TABLE email_templates (
    id BIGSERIAL PRIMARY KEY,
    user_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    html_body TEXT NOT NULL DEFAULT '',
    text_body TEXT NOT NULL DEFAULT '',
    category TEXT NOT NULL DEFAULT 'general',
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_address, name)
);
CREATE INDEX idx_email_templates_user_date
    ON email_templates(user_address, updated_at DESC);
CREATE UNIQUE INDEX idx_email_templates_user_default
    ON email_templates(user_address)
    WHERE is_default = true;

-- group-based RBAC (migrate-013)
CREATE TABLE groups (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    domain TEXT REFERENCES domains(name) ON DELETE CASCADE,
    description TEXT NOT NULL DEFAULT '',
    is_builtin BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE NULLS NOT DISTINCT (name, domain)
);
CREATE TABLE group_permissions (
    group_id BIGINT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (group_id, permission)
);
CREATE TABLE account_groups (
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    group_id BIGINT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    PRIMARY KEY (account_address, group_id)
);
CREATE TABLE account_permission_overrides (
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    granted BOOLEAN NOT NULL,
    PRIMARY KEY (account_address, permission)
);
CREATE INDEX idx_account_groups_address ON account_groups(account_address);
CREATE INDEX idx_account_groups_group ON account_groups(group_id);
CREATE INDEX idx_group_permissions_group ON group_permissions(group_id);
CREATE INDEX idx_groups_domain ON groups(domain);

-- seed: builtin super group. permission list = migrate-013 base set
-- + internal.rpc (014) + admin.oauth_clients (029) + admin.system_config (030)
INSERT INTO groups (name, domain, description, is_builtin)
VALUES ('super', NULL, 'Full access to all resources across all domains', true);
INSERT INTO group_permissions (group_id, permission)
SELECT g.id, p.perm
FROM groups g,
     UNNEST(ARRAY[
         'mail.send', 'mail.read', 'mail.read_domain',
         'admin.domains', 'admin.accounts', 'admin.aliases',
         'admin.groups', 'admin.queue', 'admin.sieve', 'admin.impersonate',
         'internal.rpc', 'admin.oauth_clients', 'admin.system_config'
     ]) AS p(perm)
WHERE g.name = 'super' AND g.domain IS NULL;

-- app registration: API clients with scoped permissions (migrate-014)
CREATE TABLE apps (
    id BIGSERIAL PRIMARY KEY,
    app_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    owner_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    scopes TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_apps_owner ON apps(owner_address);

-- API keys (migrate-010/011; app_id link from 014)
CREATE TABLE api_keys (
    id              BIGSERIAL PRIMARY KEY,
    prefix          TEXT NOT NULL UNIQUE,
    key_hash        TEXT NOT NULL,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name            TEXT NOT NULL DEFAULT '',
    expires_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    full_key        TEXT,
    app_id          BIGINT REFERENCES apps(id) ON DELETE CASCADE
);
CREATE INDEX idx_api_keys_prefix_active ON api_keys(prefix) WHERE revoked_at IS NULL;
CREATE INDEX idx_api_keys_account ON api_keys(account_address) WHERE revoked_at IS NULL;
CREATE INDEX idx_api_keys_app ON api_keys(app_id) WHERE app_id IS NOT NULL;

-- distribution lists (migrate-015)
CREATE TABLE email_groups (
    id BIGSERIAL PRIMARY KEY,
    address TEXT NOT NULL UNIQUE,
    domain TEXT NOT NULL REFERENCES domains(name) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE email_group_members (
    group_id BIGINT NOT NULL REFERENCES email_groups(id) ON DELETE CASCADE,
    member_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    PRIMARY KEY (group_id, member_address)
);
CREATE INDEX idx_email_groups_domain ON email_groups(domain);
CREATE INDEX idx_email_group_members_member ON email_group_members(member_address);

-- admin action audit trail (migrate-017)
CREATE TABLE audit_log (
    id BIGSERIAL PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT '',
    ip_address TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_audit_log_actor ON audit_log(actor);
CREATE INDEX idx_audit_log_timestamp ON audit_log(timestamp);

-- per-account signatures (migrate-018)
CREATE TABLE signatures (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'Default',
    html TEXT NOT NULL DEFAULT '',
    text_content TEXT NOT NULL DEFAULT '',
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_signatures_account ON signatures(account_address);

-- password reset flow (migrate-019)
CREATE TABLE password_reset_tokens (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_password_reset_token ON password_reset_tokens(token);

-- TOTP 2FA secrets (migrate-021)
CREATE TABLE totp_secrets (
    account_address TEXT PRIMARY KEY REFERENCES accounts(address) ON DELETE CASCADE,
    secret TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    recovery_codes TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- PGP / S/MIME public keys (migrate-022)
CREATE TABLE encryption_keys (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    key_type TEXT NOT NULL CHECK (key_type IN ('pgp', 'smime')),
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, key_type)
);
CREATE INDEX idx_encryption_keys_account ON encryption_keys(account_address);

-- CalDAV calendars (migrate-023; is_external_readonly from 034)
CREATE TABLE calendars (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'Default',
    color TEXT NOT NULL DEFAULT '#4285f4',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_external_readonly BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(account_address, name)
);

-- calendar events (migrate-023; iTIP columns from 031; per-instance
-- RECURRENCE-ID uniqueness from 033 replaces the original
-- UNIQUE(calendar_id, uid) constraint)
CREATE TABLE calendar_events (
    id BIGSERIAL PRIMARY KEY,
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    etag TEXT NOT NULL,
    icalendar TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    dtstart TIMESTAMPTZ,
    dtend TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    organizer TEXT,
    attendees JSONB NOT NULL DEFAULT '[]'::jsonb,
    sequence INT NOT NULL DEFAULT 0,
    dtstamp TIMESTAMPTZ,
    status TEXT,
    method TEXT,
    rrule TEXT,
    recurrence_id TIMESTAMPTZ,
    last_modified TIMESTAMPTZ
);
CREATE INDEX idx_calendar_events_calendar ON calendar_events(calendar_id);
CREATE INDEX idx_calendar_events_uid ON calendar_events(uid);
CREATE INDEX idx_calendar_events_organizer
    ON calendar_events(organizer)
    WHERE organizer IS NOT NULL;
CREATE INDEX idx_calendar_events_active_dtstart
    ON calendar_events(calendar_id, dtstart)
    WHERE status IS DISTINCT FROM 'CANCELLED' AND dtstart IS NOT NULL;
CREATE INDEX idx_calendar_events_uid_seq
    ON calendar_events(uid, sequence DESC);
CREATE UNIQUE INDEX uq_calendar_events_master
    ON calendar_events(calendar_id, uid)
    WHERE recurrence_id IS NULL;
CREATE UNIQUE INDEX uq_calendar_events_instance
    ON calendar_events(calendar_id, uid, recurrence_id)
    WHERE recurrence_id IS NOT NULL;

-- CardDAV address books + vCard contacts (migrate-023/042 — the name
-- `contacts` is the CardDAV table; message-derived stats live in
-- email_contacts above)
CREATE TABLE address_books (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT 'Default',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, name)
);
CREATE TABLE contacts (
    id BIGSERIAL PRIMARY KEY,
    address_book_id BIGINT NOT NULL REFERENCES address_books(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    etag TEXT NOT NULL,
    vcard TEXT NOT NULL,
    fn_name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(address_book_id, uid)
);
CREATE INDEX idx_contacts_book ON contacts(address_book_id);
CREATE INDEX idx_contacts_uid ON contacts(uid);

-- bounce suppression list (migrate-027)
CREATE TABLE suppression_list (
    id BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    bounce_type TEXT NOT NULL DEFAULT 'hard',
    smtp_code INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_suppression_email ON suppression_list (email);
CREATE INDEX idx_suppression_created ON suppression_list (created_at DESC);

-- user spam/ham feedback (migrate-028)
CREATE TABLE spam_feedback (
    id BIGSERIAL PRIMARY KEY,
    user_address TEXT NOT NULL,
    message_id TEXT NOT NULL,
    label TEXT NOT NULL CHECK (label IN ('spam', 'ham')),
    subject TEXT,
    sender TEXT,
    features JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_spam_feedback_label ON spam_feedback (label);
CREATE INDEX idx_spam_feedback_created ON spam_feedback (created_at DESC);

-- OIDC provider (migrate-029; redirect_uris TEXT[] is the post-D-pre form)
CREATE TABLE oauth_clients (
    client_id TEXT PRIMARY KEY,
    secret_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    redirect_uris TEXT[] NOT NULL,
    scopes TEXT NOT NULL DEFAULT 'openid email profile',
    trusted BOOLEAN NOT NULL DEFAULT false,
    active BOOLEAN NOT NULL DEFAULT true,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE oauth_auth_codes (
    code TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    scopes TEXT NOT NULL,
    code_challenge TEXT,
    code_challenge_method TEXT,
    nonce TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_oauth_auth_codes_expires ON oauth_auth_codes(expires_at);
CREATE TABLE oauth_signing_keys (
    kid TEXT PRIMARY KEY,
    public_key_pem TEXT NOT NULL,
    private_key_pem TEXT NOT NULL,
    algorithm TEXT NOT NULL DEFAULT 'RS256',
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE oauth_refresh_tokens (
    token_hash TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    scopes TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_oauth_refresh_tokens_expires ON oauth_refresh_tokens(expires_at);

-- external ICS feed subscriptions (migrate-034)
CREATE TABLE external_calendar_feeds (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    etag TEXT,
    last_modified TEXT,
    basic_auth_user TEXT,
    basic_auth_pass TEXT,
    refresh_interval_secs INT NOT NULL DEFAULT 900,
    last_synced_at TIMESTAMPTZ,
    last_error TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, url)
);
CREATE INDEX idx_external_feeds_active_due
    ON external_calendar_feeds(enabled, last_synced_at)
    WHERE enabled = TRUE;

-- TLSRPT event facts, RFC 8460 (migrate-036)
CREATE TABLE tls_rpt_events (
    id BIGSERIAL PRIMARY KEY,
    recorded_at_unix BIGINT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('success', 'failure')),
    policy_domain TEXT NOT NULL,
    policy_type TEXT NOT NULL,
    mx_host TEXT,
    result_type TEXT,
    sending_mta_ip TEXT,
    receiving_ip TEXT,
    receiving_mx_helo TEXT,
    additional_information TEXT,
    failure_reason_code TEXT
);
CREATE INDEX tls_rpt_events_recorded_at_idx
    ON tls_rpt_events (recorded_at_unix);

-- performance indexes historically added by migrate-004/005/006
CREATE INDEX idx_accounts_domain ON accounts(domain);
CREATE INDEX idx_greylist_last_seen ON greylist_triplets(last_seen);
CREATE INDEX idx_ea_risk ON email_analysis(risk_score) WHERE risk_score > 0;
CREATE INDEX idx_queue_domain ON outbound_queue(domain) WHERE status = 'pending';
CREATE INDEX idx_messages_pinned ON messages(thread_id, pinned) WHERE pinned = true;
CREATE INDEX idx_messages_archived ON messages(thread_id, archived) WHERE archived = true;
CREATE INDEX idx_messages_sender ON messages(sender);
CREATE INDEX idx_messages_thread_date ON messages(thread_id, internal_date DESC);
CREATE INDEX idx_messages_size_nonzero ON messages(id DESC) WHERE size > 0;
CREATE INDEX idx_messages_sender_trgm ON messages USING gin (sender gin_trgm_ops)
    WHERE sender IS NOT NULL AND sender != '';
CREATE INDEX idx_messages_subject_trgm ON messages USING gin (subject gin_trgm_ops)
    WHERE subject IS NOT NULL AND subject != '';
CREATE INDEX idx_messages_text_body_trgm ON messages USING gin (text_body gin_trgm_ops)
    WHERE text_body IS NOT NULL AND text_body != '';
CREATE INDEX idx_messages_clean_text_trgm ON messages USING gin (clean_text gin_trgm_ops)
    WHERE clean_text IS NOT NULL AND clean_text != '';
