-- migrate embedding column from 768 to 1024 dimensions (qwen3-embedding)
-- clear existing embeddings since they're from a different model
ALTER TABLE email_analysis
    ALTER COLUMN embedding TYPE vector(1024)
    USING NULL;
