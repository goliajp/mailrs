-- migrate-060: reactions are gone from the product (no route, no reader);
-- their table goes too. Mirrored in init-schema.sql.
DROP TABLE IF EXISTS reactions;
