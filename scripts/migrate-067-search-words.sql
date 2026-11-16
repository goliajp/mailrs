-- migrate-067: the words conversation search matches, cut by the
-- application with the tokenizer the kevy store indexes with, so the two
-- stores find the same conversations: a message's subject, sender and
-- preview; its body; an attachment's text. NULL is mail recorded before
-- them, which the SQL core fills in when it starts. Mirrored in
-- init-schema.sql.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS header_words tsvector;
ALTER TABLE messages ADD COLUMN IF NOT EXISTS body_words tsvector;
CREATE INDEX IF NOT EXISTS idx_messages_header_words ON messages USING GIN (header_words);
CREATE INDEX IF NOT EXISTS idx_messages_body_words ON messages USING GIN (body_words);
CREATE INDEX IF NOT EXISTS idx_messages_header_words_null ON messages (id) WHERE header_words IS NULL;
CREATE INDEX IF NOT EXISTS idx_messages_snippet_trgm ON messages USING gin (snippet gin_trgm_ops)
    WHERE snippet != '';
ALTER TABLE attachment_content ADD COLUMN IF NOT EXISTS words tsvector;
CREATE INDEX IF NOT EXISTS idx_attachment_content_words ON attachment_content USING GIN (words);
