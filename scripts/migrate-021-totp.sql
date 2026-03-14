CREATE TABLE IF NOT EXISTS totp_secrets (
    account_address TEXT PRIMARY KEY REFERENCES accounts(address) ON DELETE CASCADE,
    secret TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    recovery_codes TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
