-- bounce suppression list: addresses that permanently rejected delivery
CREATE TABLE IF NOT EXISTS suppression_list (
    id BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    bounce_type TEXT NOT NULL DEFAULT 'hard', -- 'hard' or 'complaint'
    smtp_code INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_suppression_email ON suppression_list (email);
CREATE INDEX IF NOT EXISTS idx_suppression_created ON suppression_list (created_at DESC);
