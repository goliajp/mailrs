-- add clean_text column to email_analysis for AI-extracted readable text
ALTER TABLE email_analysis ADD COLUMN IF NOT EXISTS clean_text TEXT NOT NULL DEFAULT '';
