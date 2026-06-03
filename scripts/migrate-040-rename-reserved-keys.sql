-- migrate-040-rename-reserved-keys.sql
-- Phase D-pre #4: rename two columns named `key` to avoid SPG SQL parser
-- treating `key` as a reserved word (SPG v7.9 errors with
-- "unsupported column type \"key\"").
--
-- Idempotent: both renames guarded by information_schema lookup.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'greylist_triplets'
          AND column_name = 'key'
    ) THEN
        ALTER TABLE greylist_triplets RENAME COLUMN key TO triplet;
    END IF;

    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'system_config'
          AND column_name = 'key'
    ) THEN
        ALTER TABLE system_config RENAME COLUMN key TO config_key;
    END IF;
END $$;
