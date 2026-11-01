-- migrate-058: why each recipient's copy of a message was filed where it
-- was (their sender rule, the classifier, their filter), beside the verdict
-- every recipient shares. Written once on delivery. Mirrored in
-- init-schema.sql.
CREATE TABLE IF NOT EXISTS message_placements (
    account_address TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    placement       JSONB NOT NULL,
    PRIMARY KEY (account_address, message_id)
);
