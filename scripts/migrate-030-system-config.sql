-- system config: runtime-editable configuration stored in database
CREATE TABLE IF NOT EXISTS system_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    value_type TEXT NOT NULL DEFAULT 'string',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL DEFAULT ''
);

-- grant admin.system_config to super group
INSERT INTO group_permissions (group_id, permission)
SELECT g.id, 'admin.system_config'
FROM groups g
WHERE g.name = 'super' AND g.domain IS NULL
ON CONFLICT DO NOTHING;
