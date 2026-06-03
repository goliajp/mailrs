-- migrate-038-oauth-redirect-uris-csv.sql
-- Phase D-pre #2: convert oauth_clients.redirect_uris from TEXT[] to CSV TEXT.
-- SPG (v7.9) has no array column type; storing as comma-separated TEXT lets
-- the same schema work on both PG and SPG.
--
-- Idempotent: the column rename is conditional on the array column still
-- existing. Re-running this migration on a database already converted is a
-- no-op.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'oauth_clients'
          AND column_name = 'redirect_uris'
          AND data_type = 'ARRAY'
    ) THEN
        ALTER TABLE oauth_clients RENAME COLUMN redirect_uris TO redirect_uris_arr;
        ALTER TABLE oauth_clients ADD COLUMN redirect_uris TEXT NOT NULL DEFAULT '';
        UPDATE oauth_clients SET redirect_uris = array_to_string(redirect_uris_arr, ',');
        ALTER TABLE oauth_clients DROP COLUMN redirect_uris_arr;
    END IF;
END $$;
