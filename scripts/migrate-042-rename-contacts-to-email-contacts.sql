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
-- The first v1.7.106 version of this migration assumed `email_contacts`
-- would not already exist on prod-upgrade. That was wrong: deploy.sh runs
-- migrate-009 (now also using the new `email_contacts` name) BEFORE
-- migrate-042, so by the time we get here `email_contacts` exists as an
-- empty side-effect table. We detect and drop that empty side-effect
-- before renaming.
--
-- Idempotent across both paths:
--   * fresh build  : contacts is CardDAV-shaped (or absent), skip the rename
--   * legacy prod  : contacts is email-stats-shaped (user_address column);
--                    email_contacts might exist as an empty side-effect
--                    from migrate-009 — drop it (with safety check), then
--                    rename and let CardDAV contacts get materialised below
--   * re-run       : everything already done — both branches no-op

DO $$
DECLARE
    has_user_address BOOLEAN;
    ec_exists BOOLEAN;
    ec_rows BIGINT := 0;
BEGIN
    SELECT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'contacts'
          AND column_name = 'user_address'
    ) INTO has_user_address;

    IF NOT has_user_address THEN
        -- contacts is already CardDAV-shaped (or absent). Nothing to do
        -- in this branch — the CREATE TABLE IF NOT EXISTS below covers
        -- the absent case.
        RETURN;
    END IF;

    -- legacy prod path. May have a side-effect empty email_contacts from
    -- the new migrate-009 having run earlier in this same deploy.
    SELECT EXISTS (
        SELECT 1 FROM information_schema.tables WHERE table_name = 'email_contacts'
    ) INTO ec_exists;

    IF ec_exists THEN
        SELECT count(*) INTO ec_rows FROM email_contacts;
        IF ec_rows <> 0 THEN
            RAISE EXCEPTION 'migrate-042: email_contacts already has % rows alongside legacy contacts; refusing to clobber. Manual rescue required.', ec_rows;
        END IF;
        DROP TABLE email_contacts;
    END IF;

    ALTER TABLE contacts RENAME TO email_contacts;
    ALTER INDEX IF EXISTS idx_contacts_user_score RENAME TO idx_email_contacts_user_score;
    ALTER INDEX IF EXISTS idx_contacts_user_email RENAME TO idx_email_contacts_user_email;
END $$;

-- Now `contacts` is free (or already CardDAV); build the CardDAV table
-- mirroring migrate-023's definition. CREATE TABLE IF NOT EXISTS so re-runs
-- are safe and a previously-correctly-built CardDAV contacts (e.g. fresh
-- SPG via init-schema-spg.sql) is left alone.
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
