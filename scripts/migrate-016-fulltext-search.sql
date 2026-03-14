-- migrate-016-fulltext-search.sql
-- full-text search via tsvector + GIN index

-- add search_vector column
ALTER TABLE messages ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- function to build search vector from message fields
-- uses 'simple' config for multilingual support (no language-specific stemming)
CREATE OR REPLACE FUNCTION messages_search_vector_update() RETURNS trigger AS $$
BEGIN
  NEW.search_vector :=
    setweight(to_tsvector('simple', COALESCE(NEW.subject, '')), 'A') ||
    setweight(to_tsvector('simple', COALESCE(NEW.sender, '')), 'B') ||
    setweight(to_tsvector('simple', COALESCE(NEW.recipients, '')), 'B') ||
    setweight(to_tsvector('simple', COALESCE(NEW.clean_text, COALESCE(NEW.text_body, ''))), 'C');
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- trigger to auto-update on insert/update
DROP TRIGGER IF EXISTS messages_search_vector_trigger ON messages;
CREATE TRIGGER messages_search_vector_trigger
  BEFORE INSERT OR UPDATE OF subject, sender, recipients, text_body, clean_text
  ON messages
  FOR EACH ROW
  EXECUTE FUNCTION messages_search_vector_update();

-- GIN index for fast full-text search
CREATE INDEX IF NOT EXISTS idx_messages_search_vector ON messages USING GIN (search_vector);

-- backfill existing rows
UPDATE messages SET search_vector =
  setweight(to_tsvector('simple', COALESCE(subject, '')), 'A') ||
  setweight(to_tsvector('simple', COALESCE(sender, '')), 'B') ||
  setweight(to_tsvector('simple', COALESCE(recipients, '')), 'B') ||
  setweight(to_tsvector('simple', COALESCE(clean_text, COALESCE(text_body, ''))), 'C')
WHERE search_vector IS NULL;
