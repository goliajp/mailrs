-- migrate-042-rename-contacts-to-email-contacts.sql
--
-- Phase D-cutover #3: free the `contacts` name for the CardDAV vCard table.
--
-- History:
--   * migrate-009 (and init-schema before it) created `contacts` as a
--     per-user message-derived sender/recipient stats table (columns:
--     user_address, email, relationship_score, importance_bias, etc.).
--   * migrate-023 then tried to `CREATE TABLE IF NOT EXISTS contacts` for
--     CardDAV vCard objects (columns: address_book_id, uid, etag, vcard, …).
--     PG silently skipped the second create because the name already existed,
--     so on PG the CardDAV `contacts` never actually got built — CardDAV PUT
--     would fail at the storage layer if anyone tried.
--   * SPG (D-validate round 9) loaded a prod dump and hit 998 NOT NULL
--     violations on `address_book_id` because SPG's schema-creation order
--     surfaced the conflict differently. The fix is the same on either
--     engine: rename the legacy table to `email_contacts`, then materialise
--     the CardDAV `contacts` properly.
--
-- Idempotent: detects the legacy shape (user_address column) before doing
-- anything. Re-running on an already-converted database is a no-op.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'contacts'
          AND column_name = 'user_address'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_name = 'email_contacts'
    ) THEN
        ALTER TABLE contacts RENAME TO email_contacts;
        ALTER INDEX IF EXISTS idx_contacts_user_score RENAME TO idx_email_contacts_user_score;
        ALTER INDEX IF EXISTS idx_contacts_user_email RENAME TO idx_email_contacts_user_email;
    END IF;
END $$;

-- Now `contacts` is free; build the CardDAV table mirroring migrate-023's
-- definition. CREATE TABLE IF NOT EXISTS so re-runs are safe and a
-- previously-correctly-built CardDAV contacts (e.g. fresh SPG via
-- init-schema-spg.sql) is left alone.
CREATE TABLE IF NOT EXISTS contacts (
    id BIGSERIAL PRIMARY KEY,
    address_book_id BIGINT NOT NULL REFERENCES address_books(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    etag TEXT NOT NULL,
    vcard TEXT NOT NULL,
    fn_name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(address_book_id, uid)
);
CREATE INDEX IF NOT EXISTS idx_contacts_book ON contacts(address_book_id);
CREATE INDEX IF NOT EXISTS idx_contacts_uid ON contacts(uid);
