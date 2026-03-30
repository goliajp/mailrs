CREATE TABLE IF NOT EXISTS oauth_clients (
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

CREATE TABLE IF NOT EXISTS oauth_auth_codes (
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
CREATE INDEX IF NOT EXISTS idx_oauth_auth_codes_expires ON oauth_auth_codes(expires_at);

CREATE TABLE IF NOT EXISTS oauth_signing_keys (
    kid TEXT PRIMARY KEY,
    public_key_pem TEXT NOT NULL,
    private_key_pem TEXT NOT NULL,
    algorithm TEXT NOT NULL DEFAULT 'RS256',
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS oauth_refresh_tokens (
    token_hash TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    scopes TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_oauth_refresh_tokens_expires ON oauth_refresh_tokens(expires_at);

-- Add admin.oauth_clients permission to super group
INSERT INTO group_permissions (group_id, permission)
SELECT g.id, 'admin.oauth_clients'
FROM groups g
WHERE g.name = 'super' AND g.domain IS NULL
ON CONFLICT DO NOTHING;
