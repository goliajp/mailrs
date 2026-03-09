CREATE TABLE IF NOT EXISTS api_keys (
    id              BIGSERIAL PRIMARY KEY,
    prefix          TEXT NOT NULL UNIQUE,
    key_hash        TEXT NOT NULL,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    name            TEXT NOT NULL DEFAULT '',
    expires_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_prefix_active ON api_keys(prefix) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_api_keys_account ON api_keys(account_address) WHERE revoked_at IS NULL;
