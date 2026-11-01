-- migrate-057: what delivery records about each message for the sectioned
-- inbox, and the senders a reader accepted. NULL facts are mail from before
-- they were recorded: a conversation none of whose messages carries them has
-- no section yet. Mirrored in init-schema.sql.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS direct BOOLEAN;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS from_person BOOLEAN;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS first_contact BOOLEAN;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS to_others BOOLEAN;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS from_addr TEXT NOT NULL DEFAULT '';
-- A fact per acceptance, appended: a first contact accepted stops being new.
CREATE TABLE IF NOT EXISTS accepted_senders (
    account_address TEXT NOT NULL,
    address         TEXT NOT NULL,
    accepted_at     BIGINT NOT NULL,
    PRIMARY KEY (account_address, address)
);
