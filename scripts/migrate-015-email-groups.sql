-- migrate-015-email-groups.sql
-- group email: distribution lists where all members receive a copy

CREATE TABLE IF NOT EXISTS email_groups (
    id BIGSERIAL PRIMARY KEY,
    address TEXT NOT NULL UNIQUE,
    domain TEXT NOT NULL REFERENCES domains(name) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS email_group_members (
    group_id BIGINT NOT NULL REFERENCES email_groups(id) ON DELETE CASCADE,
    member_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    PRIMARY KEY (group_id, member_address)
);

CREATE INDEX IF NOT EXISTS idx_email_groups_domain ON email_groups(domain);
CREATE INDEX IF NOT EXISTS idx_email_group_members_member ON email_group_members(member_address);
