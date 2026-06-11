-- migrate-044: D-pre #2 revert — oauth_clients.redirect_uris CSV TEXT -> TEXT[]
--
-- Phase D-pre #2 (migrate-038) flattened the column to comma-separated
-- TEXT because SPG v7.9 had no array types. The engine has shipped
-- TEXT[] end-to-end since v7.10.9 (round-12 added native array binds),
-- so the PG-original shape comes back. Inverse of migrate-038.
ALTER TABLE oauth_clients
    ALTER COLUMN redirect_uris TYPE TEXT[]
    USING string_to_array(redirect_uris, ',');
