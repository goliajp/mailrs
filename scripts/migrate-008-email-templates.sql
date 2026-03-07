-- user email templates
CREATE TABLE IF NOT EXISTS email_templates (
    id BIGSERIAL PRIMARY KEY,
    -- reference accounts so templates are removed when an account is deleted
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

-- the UNIQUE(user_address, name) constraint creates an implicit index, but an
-- explicit partial index on (user_address, updated_at) accelerates the
-- ORDER BY is_default DESC, updated_at DESC list query
CREATE INDEX IF NOT EXISTS idx_email_templates_user_date
    ON email_templates(user_address, updated_at DESC);

-- ensure at most one default template per user
CREATE UNIQUE INDEX IF NOT EXISTS idx_email_templates_user_default
    ON email_templates(user_address)
    WHERE is_default = true;
