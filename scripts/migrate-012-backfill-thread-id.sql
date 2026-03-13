-- backfill thread_id for messages that have empty thread_id
-- generates a synthetic id from internal_date + message row id
UPDATE messages
SET thread_id = internal_date || '.' || id || '@mailrs.local',
    message_id = CASE WHEN message_id = '' THEN internal_date || '.' || id || '@mailrs.local' ELSE message_id END
WHERE thread_id = '';

-- add trigram index on clean_text for ILIKE search
CREATE INDEX IF NOT EXISTS idx_messages_clean_text_trgm
    ON messages
    USING gin(clean_text gin_trgm_ops)
    WHERE clean_text IS NOT NULL AND clean_text != '';
