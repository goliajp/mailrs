-- migrate-039-outbound-message-base64.sql
-- Phase D-pre #3: convert outbound_queue.message_data from BYTEA to base64 TEXT.
-- SPG v7.9 has no BYTES type (slip to v7.10+); storing as base64-standard TEXT
-- lets the same schema work on both PG and SPG.
--
-- Idempotent: column transformation is conditional on the BYTEA column still
-- existing. Re-running this migration on a database already converted is a
-- no-op.
--
-- Note: base64 expands payload size by ~33% on disk. For mailrs's outbound
-- queue this is acceptable — rows are short-lived (queued, delivered or
-- bounced within minutes) and the volume is small (low thousands/day even
-- for a busy domain).

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'outbound_queue'
          AND column_name = 'message_data'
          AND data_type = 'bytea'
    ) THEN
        ALTER TABLE outbound_queue RENAME COLUMN message_data TO message_data_bin;
        ALTER TABLE outbound_queue ADD COLUMN message_data TEXT NOT NULL DEFAULT '';
        UPDATE outbound_queue SET message_data = encode(message_data_bin, 'base64');
        ALTER TABLE outbound_queue DROP COLUMN message_data_bin;
    END IF;
END $$;
