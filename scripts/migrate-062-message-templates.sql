-- migrate-062: each delivered message's template fingerprint, once per
-- message, for finding the other copies of a reported scam. Derived: the
-- SimHash of the body and the link domains, attachments and sender domain
-- it is compared on, all computable again from the file. The four 16-bit
-- pieces are what a report looks a fingerprint up by; they are kept only
-- while the message is inside the thirty days a report looks back over
-- and set to NULL after, so their indexes stay that size. Mirrored in
-- init-schema.sql.
CREATE TABLE IF NOT EXISTS message_templates (
    message_id    TEXT PRIMARY KEY,
    simhash       BIGINT NOT NULL,
    arrived       BIGINT NOT NULL,
    links         TEXT NOT NULL DEFAULT '',
    files         TEXT NOT NULL DEFAULT '',
    sender        TEXT NOT NULL DEFAULT '',
    s0            INTEGER,
    s1            INTEGER,
    s2            INTEGER,
    s3            INTEGER
);
CREATE INDEX IF NOT EXISTS idx_message_templates_s0 ON message_templates(s0);
CREATE INDEX IF NOT EXISTS idx_message_templates_s1 ON message_templates(s1);
CREATE INDEX IF NOT EXISTS idx_message_templates_s2 ON message_templates(s2);
CREATE INDEX IF NOT EXISTS idx_message_templates_s3 ON message_templates(s3);
CREATE INDEX IF NOT EXISTS idx_message_templates_arrived ON message_templates(arrived);
