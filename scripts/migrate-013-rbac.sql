-- migrate-013-rbac.sql
-- group-based RBAC permission system
-- replaces the old super_domains column on accounts

-- groups: permission containers, optionally scoped to a domain
CREATE TABLE IF NOT EXISTS groups (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    domain TEXT REFERENCES domains(name) ON DELETE CASCADE,
    description TEXT NOT NULL DEFAULT '',
    is_builtin BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE NULLS NOT DISTINCT (name, domain)
);

-- group_permissions: which permissions a group grants
CREATE TABLE IF NOT EXISTS group_permissions (
    group_id BIGINT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (group_id, permission)
);

-- account_groups: many-to-many between accounts and groups
CREATE TABLE IF NOT EXISTS account_groups (
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    group_id BIGINT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    PRIMARY KEY (account_address, group_id)
);

-- account_permission_overrides: per-account grant/revoke
CREATE TABLE IF NOT EXISTS account_permission_overrides (
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    granted BOOLEAN NOT NULL,
    PRIMARY KEY (account_address, permission)
);

-- seed: super group (global, all permissions)
INSERT INTO groups (name, domain, description, is_builtin)
VALUES ('super', NULL, 'Full access to all resources across all domains', true)
ON CONFLICT DO NOTHING;

INSERT INTO group_permissions (group_id, permission)
SELECT g.id, p.perm
FROM groups g,
     UNNEST(ARRAY[
         'mail.send', 'mail.read', 'mail.read_domain',
         'admin.domains', 'admin.accounts', 'admin.aliases',
         'admin.groups', 'admin.queue', 'admin.sieve', 'admin.impersonate'
     ]) AS p(perm)
WHERE g.name = 'super' AND g.domain IS NULL
ON CONFLICT DO NOTHING;

-- seed: default user group per domain
INSERT INTO groups (name, domain, description, is_builtin)
SELECT 'user', name, 'Default user group for ' || name, true
FROM domains
ON CONFLICT DO NOTHING;

INSERT INTO group_permissions (group_id, permission)
SELECT g.id, p.perm
FROM groups g,
     UNNEST(ARRAY['mail.send', 'mail.read']) AS p(perm)
WHERE g.name = 'user'
ON CONFLICT DO NOTHING;

-- migrate: accounts with super_domains -> super group
INSERT INTO account_groups (account_address, group_id)
SELECT a.address, g.id
FROM accounts a, groups g
WHERE g.name = 'super' AND g.domain IS NULL
  AND a.super_domains != ''
ON CONFLICT DO NOTHING;

-- migrate: all other accounts -> their domain's user group
INSERT INTO account_groups (account_address, group_id)
SELECT a.address, g.id
FROM accounts a
JOIN groups g ON g.name = 'user' AND g.domain = a.domain
WHERE a.address NOT IN (
    SELECT account_address FROM account_groups
)
ON CONFLICT DO NOTHING;

-- drop the old column
ALTER TABLE accounts DROP COLUMN IF EXISTS super_domains;

-- indexes
CREATE INDEX IF NOT EXISTS idx_account_groups_address ON account_groups(account_address);
CREATE INDEX IF NOT EXISTS idx_account_groups_group ON account_groups(group_id);
CREATE INDEX IF NOT EXISTS idx_group_permissions_group ON group_permissions(group_id);
CREATE INDEX IF NOT EXISTS idx_groups_domain ON groups(domain);
