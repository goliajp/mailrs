-- BIMI (Brand Indicators for Message Identification) support
ALTER TABLE messages ADD COLUMN IF NOT EXISTS bimi_logo_url TEXT;
