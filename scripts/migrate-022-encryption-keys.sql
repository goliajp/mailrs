-- encryption keys: PGP public keys and S/MIME certificates per account
CREATE TABLE IF NOT EXISTS encryption_keys (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    key_type TEXT NOT NULL CHECK (key_type IN ('pgp', 'smime')),
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, key_type)
);
CREATE INDEX IF NOT EXISTS idx_encryption_keys_account ON encryption_keys(account_address);
