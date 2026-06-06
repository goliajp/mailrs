-- P1: email_contacts table (renamed from `contacts` in migrate-042 to free
-- the `contacts` name for the CardDAV table created by migrate-023).
CREATE TABLE IF NOT EXISTS email_contacts (
    id              BIGSERIAL PRIMARY KEY,
    user_address    TEXT NOT NULL,
    email           TEXT NOT NULL,
    display_name    TEXT NOT NULL DEFAULT '',

    -- auto stats
    first_seen      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen       TIMESTAMPTZ NOT NULL DEFAULT now(),
    received_count  INT NOT NULL DEFAULT 0,
    sent_count      INT NOT NULL DEFAULT 0,
    reply_count     INT NOT NULL DEFAULT 0,

    -- classification
    is_mutual       BOOLEAN NOT NULL DEFAULT false,
    is_mailing_list BOOLEAN NOT NULL DEFAULT false,
    is_automated    BOOLEAN NOT NULL DEFAULT false,

    -- extracted info
    organization    TEXT NOT NULL DEFAULT '',
    title           TEXT NOT NULL DEFAULT '',
    phone           TEXT NOT NULL DEFAULT '',

    -- user settings
    importance_bias REAL NOT NULL DEFAULT 0.0,
    is_vip          BOOLEAN NOT NULL DEFAULT false,
    is_blocked      BOOLEAN NOT NULL DEFAULT false,

    -- computed
    relationship_score REAL NOT NULL DEFAULT 0.0,

    UNIQUE(user_address, email)
);
CREATE INDEX IF NOT EXISTS idx_email_contacts_user_score ON email_contacts(user_address, relationship_score DESC);
CREATE INDEX IF NOT EXISTS idx_email_contacts_user_email ON email_contacts(user_address, email);

-- P2+P4: messages table extensions
ALTER TABLE messages ADD COLUMN IF NOT EXISTS html_body TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS clean_text TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS new_content TEXT;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS importance_level TEXT NOT NULL DEFAULT 'normal';
ALTER TABLE messages ADD COLUMN IF NOT EXISTS importance_score REAL NOT NULL DEFAULT 0.0;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS is_bulk_sender BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS has_tracking_pixel BOOLEAN NOT NULL DEFAULT false;

-- P5: email_analysis extensions
ALTER TABLE email_analysis ADD COLUMN IF NOT EXISTS requires_action BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE email_analysis ADD COLUMN IF NOT EXISTS action_deadline TIMESTAMPTZ;
ALTER TABLE email_analysis ADD COLUMN IF NOT EXISTS sender_intent TEXT NOT NULL DEFAULT 'inform';
ALTER TABLE email_analysis ADD COLUMN IF NOT EXISTS clean_text TEXT NOT NULL DEFAULT '';

-- handle case where clean_text already exists from migrate-002
-- (the ADD COLUMN IF NOT EXISTS handles this safely)

-- P9: sender feedback for learning
CREATE TABLE IF NOT EXISTS sender_feedback (
    id              BIGSERIAL PRIMARY KEY,
    user_address    TEXT NOT NULL,
    sender_email    TEXT NOT NULL,
    action          TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_sender_feedback_user ON sender_feedback(user_address, sender_email);

-- index for importance-based listing
CREATE INDEX IF NOT EXISTS idx_messages_importance ON messages(mailbox_id, importance_level, internal_date DESC);
