-- migration 032 — store iTIP invite metadata on `messages`.
--
-- When a message arrives with a `text/calendar` part (typically a
-- METHOD=REQUEST / UPDATE / CANCEL invitation from Outlook / Google /
-- Zoom etc.), the inbound pipeline parses it via `mailrs::ical` and
-- stores the structured payload here so the web client / macapp can
-- render an invite card without re-parsing the raw MIME on every load.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS invite_payload JSONB,
    ADD COLUMN IF NOT EXISTS invite_method TEXT;

-- Most common query: "list all invites in a mailbox" / "show this
-- message's invite metadata". Partial index keeps the hot path slim.
CREATE INDEX IF NOT EXISTS idx_messages_is_invite
    ON messages(mailbox_id)
    WHERE invite_payload IS NOT NULL;

-- Future-proofing: filtering by method (e.g. "show me the latest
-- UPDATE / CANCEL state") is cheap to add now.
CREATE INDEX IF NOT EXISTS idx_messages_invite_method
    ON messages(invite_method)
    WHERE invite_method IS NOT NULL;
