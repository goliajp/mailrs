-- migrate-065: attachment text as the kevy core reads it. The text of each
-- readable part stays in attachment_content (attachment_index = its place
-- among the message's readable parts); what reading the copy came to, and
-- the web addresses its QR codes carry, are computed here, one row per
-- copy, gone with the copy. The queue of copies to read and the copies the
-- reader gave up on are work state. Mirrored in init-schema.sql.
CREATE TABLE IF NOT EXISTS attachment_text_computed (
    message_id BIGINT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    version    INTEGER NOT NULL,
    parts      INTEGER NOT NULL DEFAULT 0,
    pdfs       INTEGER NOT NULL DEFAULT 0,
    images     INTEGER NOT NULL DEFAULT 0,
    stored     INTEGER NOT NULL DEFAULT 0,
    ocr        INTEGER NOT NULL DEFAULT 0,
    failed     INTEGER NOT NULL DEFAULT 0,
    too_large  INTEGER NOT NULL DEFAULT 0,
    too_small  INTEGER NOT NULL DEFAULT 0,
    truncated  INTEGER NOT NULL DEFAULT 0,
    -- one address per line, in the order read; '' when none
    qr_urls    TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS attachment_text_queue (
    user_address TEXT NOT NULL,
    message_id   TEXT NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_address, message_id)
);
CREATE TABLE IF NOT EXISTS attachment_text_set_aside (
    user_address TEXT NOT NULL,
    message_id   TEXT NOT NULL,
    PRIMARY KEY (user_address, message_id)
);
