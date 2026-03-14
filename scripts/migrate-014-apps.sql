-- migrate-014-apps.sql
-- app registration: independent API clients with scoped permissions

-- add internal.rpc to super group
INSERT INTO group_permissions (group_id, permission)
SELECT g.id, 'internal.rpc'
FROM groups g
WHERE g.name = 'super' AND g.domain IS NULL
ON CONFLICT DO NOTHING;

-- apps table
CREATE TABLE IF NOT EXISTS apps (
    id BIGSERIAL PRIMARY KEY,
    app_id TEXT NOT NULL UNIQUE,          -- public identifier (uuid)
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    owner_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    scopes TEXT NOT NULL DEFAULT '',       -- comma-separated permission list
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- link api_keys to apps (nullable = user key, non-null = app key)
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS app_id BIGINT REFERENCES apps(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_apps_owner ON apps(owner_address);
CREATE INDEX IF NOT EXISTS idx_api_keys_app ON api_keys(app_id) WHERE app_id IS NOT NULL;
