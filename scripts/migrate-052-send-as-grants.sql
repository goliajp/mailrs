-- migrate-052: addresses an administrator granted an account to send as,
-- beyond its own, its aliases and its email groups. Mirrored in
-- init-schema.sql.
CREATE TABLE IF NOT EXISTS account_send_as (
    account_address TEXT NOT NULL,
    address         TEXT NOT NULL,
    PRIMARY KEY (account_address, address)
);

-- every send and every permission check asks which aliases point at the
-- sender; idx_aliases_source leads with the other column
CREATE INDEX IF NOT EXISTS idx_aliases_target ON aliases(target_address) WHERE active = true;
