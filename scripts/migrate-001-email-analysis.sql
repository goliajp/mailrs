-- migration 001: add email_analysis table for AI-powered email analysis
-- run against an existing mailrs database

CREATE TABLE IF NOT EXISTS email_analysis (
    message_id BIGINT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    category TEXT NOT NULL DEFAULT 'general',
    risk_score SMALLINT NOT NULL DEFAULT 0,
    risk_reason TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    people JSONB NOT NULL DEFAULT '[]',
    dates JSONB NOT NULL DEFAULT '[]',
    amounts JSONB NOT NULL DEFAULT '[]',
    action_items JSONB NOT NULL DEFAULT '[]',
    embedding vector(768),
    model_version TEXT NOT NULL DEFAULT '',
    analyzed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ea_category ON email_analysis(category);
CREATE INDEX IF NOT EXISTS idx_ea_embedding ON email_analysis
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 20);
