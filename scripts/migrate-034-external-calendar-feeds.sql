-- migration 034 — external WebDAV-hosted ICS feed subscriptions (MRS-10).
--
-- mailrs subscribes to .ics URLs published over WebDAV / HTTP (e.g. a
-- company room calendar, a public team calendar, a personal export from
-- Google Calendar). A background worker periodically GETs the URL,
-- parses VEVENTs via mailrs::ical, and upserts them into a per-feed
-- read-only calendar. Existing CalDAV server infra (mailrs's own) then
-- exposes those events to subscribed native clients alongside the
-- user's own calendar.
--
-- One read-only calendar per feed; the feed row owns it (calendar_id
-- FK; ON DELETE CASCADE so removing the feed nukes the events too).

CREATE TABLE IF NOT EXISTS external_calendar_feeds (
    id BIGSERIAL PRIMARY KEY,
    account_address TEXT NOT NULL REFERENCES accounts(address) ON DELETE CASCADE,
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    -- HTTP cache validators returned by the source on the last successful
    -- fetch; used in subsequent GETs as If-None-Match / If-Modified-Since.
    etag TEXT,
    last_modified TEXT,
    -- Optional credentials. Basic Auth is fine for the v1 use case
    -- (Nextcloud / Radicale / Outlook published calendars all support it);
    -- OAuth / cookie auth lands later if anyone asks.
    basic_auth_user TEXT,
    basic_auth_pass TEXT,
    -- Polling cadence. 15 minutes is fast enough for "did the room
    -- calendar update?" without hammering the source. Honor the source's
    -- Cache-Control max-age once we surface that field.
    refresh_interval_secs INT NOT NULL DEFAULT 900,
    last_synced_at TIMESTAMPTZ,
    -- Last error message from a failed sync (for the settings UI to
    -- display "couldn't reach: 401 Unauthorized" without making the user
    -- dig through server logs).
    last_error TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(account_address, url)
);

CREATE INDEX IF NOT EXISTS idx_external_feeds_active_due
    ON external_calendar_feeds(enabled, last_synced_at)
    WHERE enabled = TRUE;

-- Mark the read-only calendar so the CalDAV PUT path refuses writes
-- (we'll honor this in MRS-10 web::dav code: 405 Method Not Allowed for
-- a calendar where is_external_readonly = TRUE).
ALTER TABLE calendars
    ADD COLUMN IF NOT EXISTS is_external_readonly BOOLEAN NOT NULL DEFAULT FALSE;
