-- migrate-046: D-pre #4 revert — restore the `key` column names
--
-- Phase D-pre #4 (migrate-040) renamed greylist_triplets.key and
-- system_config.key away from the reserved word because SPG v7.9's
-- parser rejected it. The parser takes bare `key` since v7.17
-- (round-11 fail-list), so the PG-original names come back.
-- Inverse of migrate-040.
ALTER TABLE greylist_triplets RENAME COLUMN triplet TO key;
ALTER TABLE system_config RENAME COLUMN config_key TO key;
