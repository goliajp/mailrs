-- migrate-051: what the SQL core needs to answer the core-api the way the
-- kevy core does.
--
-- Found by calling every core-api client method on both cores with the same
-- arguments and comparing the answers. Mirrored in init-schema.sql.

-- 1. Fraud verdicts, one per message, written once when the receiver's
-- verdict arrives with the spooled message. A fact: never updated.
CREATE TABLE IF NOT EXISTS fraud_verdicts (
    message_id  TEXT PRIMARY KEY,
    verdict     JSONB NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. Conversations held for review, per account. A held thread is out of
-- every list, count and search until released.
CREATE TABLE IF NOT EXISTS quarantine_holds (
    account_address TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    held_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_address, thread_id)
);

-- 3. The preview line a list shows, as delivery computed it. Before this
-- the list cut the first 80 characters out of the stored body, which a
-- message delivered through the core-api never has.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS snippet TEXT NOT NULL DEFAULT '';

-- 4. A message nobody has scored has no importance level, not "normal".
ALTER TABLE messages ALTER COLUMN importance_level SET DEFAULT '';

-- 5. Unclassified mail is inbox mail, as the kevy core calls it.
ALTER TABLE email_analysis ALTER COLUMN category SET DEFAULT 'inbox';
UPDATE email_analysis SET category = 'inbox' WHERE category = 'general';

-- 6. One default quota for a new account on either core: 10 GiB.
ALTER TABLE accounts ALTER COLUMN quota_bytes SET DEFAULT 10737418240;

-- 7. One UID sequence per user across all of the user's mailboxes: the
-- core-api names a message by (user, uid), so a UID must not repeat between
-- two mailboxes. Seeded above every UID already handed out.
CREATE TABLE IF NOT EXISTS user_uids (
    user_address TEXT PRIMARY KEY,
    last_uid     INTEGER NOT NULL
);
INSERT INTO user_uids (user_address, last_uid)
SELECT user_address, MAX(uidnext) - 1 FROM mailboxes GROUP BY user_address
ON CONFLICT (user_address) DO NOTHING;

-- 8. Which migration the database is at, so the core can bring a database
-- forward itself from here on.
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
INSERT INTO schema_version (version) SELECT 51 WHERE NOT EXISTS (SELECT 1 FROM schema_version);
