-- migrate-045: D-pre #3 revert — outbound_queue.message_data base64 TEXT -> BYTEA
--
-- Phase D-pre #3 (migrate-039) base64-flattened the column because SPG
-- v7.9 had no BYTES type. BYTEA round-trips since v7.13 (T10-A closed
-- the ::bytea surface) and large payloads since 7.23 (round-14), so the
-- PG-original shape comes back. Inverse of migrate-039.
ALTER TABLE outbound_queue
    ALTER COLUMN message_data TYPE BYTEA
    USING decode(message_data, 'base64');
