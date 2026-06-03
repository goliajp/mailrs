-- migrate-041-ivfflat-to-hnsw.sql
-- Phase D-pre #4: drop the ivfflat embedding index and rebuild with HNSW.
-- SPG only supports HNSW; this aligns PG with SPG so the schema is identical
-- on both. pgvector HNSW is a strict superset for our usage (mailrs has
-- ~hundreds of thousands of email_analysis rows, well within HNSW's
-- performance sweet spot for 1024-dim embeddings).
--
-- Idempotent: checks idx_ea_embedding's method via pg_indexes before rebuild.

DO $$
DECLARE
    current_method TEXT;
BEGIN
    SELECT am.amname INTO current_method
      FROM pg_index i
      JOIN pg_class c ON i.indexrelid = c.oid
      JOIN pg_am am ON c.relam = am.oid
     WHERE c.relname = 'idx_ea_embedding';

    IF current_method = 'ivfflat' THEN
        DROP INDEX idx_ea_embedding;
        CREATE INDEX idx_ea_embedding ON email_analysis
            USING hnsw (embedding vector_cosine_ops);
    END IF;
END $$;
