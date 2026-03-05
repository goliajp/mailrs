-- drafts table for saving email drafts
CREATE TABLE IF NOT EXISTS drafts (
  id BIGSERIAL PRIMARY KEY,
  user_address TEXT NOT NULL,
  to_addresses TEXT NOT NULL DEFAULT '',
  cc_addresses TEXT NOT NULL DEFAULT '',
  bcc_addresses TEXT NOT NULL DEFAULT '',
  subject TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '',
  reply_to_thread_id TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_drafts_user ON drafts(user_address, updated_at DESC);
