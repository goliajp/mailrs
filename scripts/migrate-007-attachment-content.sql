-- attachment content extraction results (OCR, PDF text, etc.)
CREATE TABLE IF NOT EXISTS attachment_content (
    id BIGSERIAL PRIMARY KEY,
    message_id BIGINT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    attachment_index SMALLINT NOT NULL,
    content_type TEXT NOT NULL,
    extracted_text TEXT,
    language TEXT,
    -- use double precision for confidence scores; REAL only has ~7 significant digits
    ocr_confidence DOUBLE PRECISION NOT NULL DEFAULT 0,
    page_count SMALLINT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(message_id, attachment_index)
);

-- trigram index supports ILIKE '%term%' used in search_conversations
-- requires pg_trgm extension
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_attachment_content_extracted_trgm
    ON attachment_content
    USING gin(extracted_text gin_trgm_ops)
    WHERE extracted_text IS NOT NULL AND extracted_text != '';

-- covering index: message_id lookup also covers attachment_index to avoid heap fetch
CREATE INDEX IF NOT EXISTS idx_attachment_content_message
    ON attachment_content(message_id) INCLUDE (attachment_index);

-- partial index on messages to speed up the content worker unprocessed-batch query
-- (messages with size > 0 that need content extraction)
CREATE INDEX IF NOT EXISTS idx_messages_size_nonzero
    ON messages(id DESC)
    WHERE size > 0;

-- trigram index on messages.text_body for ILIKE search in search_conversations
CREATE INDEX IF NOT EXISTS idx_messages_text_body_trgm
    ON messages
    USING gin(text_body gin_trgm_ops)
    WHERE text_body IS NOT NULL AND text_body != '';

-- trigram indexes on subject/sender for ILIKE search in search_conversations
CREATE INDEX IF NOT EXISTS idx_messages_subject_trgm
    ON messages
    USING gin(subject gin_trgm_ops)
    WHERE subject IS NOT NULL AND subject != '';

CREATE INDEX IF NOT EXISTS idx_messages_sender_trgm
    ON messages
    USING gin(sender gin_trgm_ops)
    WHERE sender IS NOT NULL AND sender != '';

-- composite index for correlated subqueries in search_conversations
-- (thread_id + internal_date DESC) avoids per-row sort
CREATE INDEX IF NOT EXISTS idx_messages_thread_date
    ON messages(thread_id, internal_date DESC);
