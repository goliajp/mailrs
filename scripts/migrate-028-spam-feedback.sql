-- user spam/ham feedback for ML training
CREATE TABLE IF NOT EXISTS spam_feedback (
    id BIGSERIAL PRIMARY KEY,
    user_address TEXT NOT NULL,
    message_id TEXT NOT NULL,
    label TEXT NOT NULL CHECK (label IN ('spam', 'ham')),
    subject TEXT,
    sender TEXT,
    features JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_spam_feedback_label ON spam_feedback (label);
CREATE INDEX IF NOT EXISTS idx_spam_feedback_created ON spam_feedback (created_at DESC);
