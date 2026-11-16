-- migrate-069: the re-judge's own records, as the kevy store keeps them
-- under the same key names: where it stands, what each round gathered,
-- the batches and the conversations in them. A string is a row with an
-- empty field; a hash is a row per field; a set is a row per member.
-- Mirrored in init-schema.sql.
CREATE TABLE IF NOT EXISTS rejudge_kv (
    key TEXT NOT NULL,
    field TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (key, field)
);
CREATE TABLE IF NOT EXISTS rejudge_sets (
    key TEXT NOT NULL,
    member TEXT NOT NULL,
    PRIMARY KEY (key, member)
);
