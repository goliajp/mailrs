-- migration 031 — extend calendar_events with iTIP-aware structured columns.
--
-- Background: migration 023 created calendar_events with raw `icalendar TEXT`
-- plus a hand-extracted `summary / dtstart / dtend` triple — enough for a
-- minimal CalDAV server but not enough to drive iTIP REPLY generation,
-- conflict detection, or SEQUENCE/DTSTAMP-aware reconciliation.
--
-- MRS-2 landed mailrs::ical (RFC 5545 parser + iTIP semantics layer);
-- MRS-3 (this migration) projects the parsed fields into structured columns
-- so MRS-4 (inbound detection), MRS-5 (web invite card), MRS-6 (RSVP write
-- back), and MRS-7 (UPDATE/CANCEL state machine) can all SQL-query directly.

ALTER TABLE calendar_events
    ADD COLUMN IF NOT EXISTS organizer TEXT,
    ADD COLUMN IF NOT EXISTS attendees JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS sequence INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS dtstamp TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS status TEXT,
    ADD COLUMN IF NOT EXISTS method TEXT,
    ADD COLUMN IF NOT EXISTS rrule TEXT,
    ADD COLUMN IF NOT EXISTS recurrence_id TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_modified TIMESTAMPTZ;

-- Status values are constrained at the application layer (CONFIRMED /
-- TENTATIVE / CANCELLED) so that a future iTIP method (e.g. POLLED) doesn't
-- require a migration to extend an enum. Same for method.

-- Indexes targeted at the queries MRS-4..MRS-9 will run:
-- - find_conflicts(account, start, end) walks active events by dtstart range
-- - organizer search for "what events did X invite me to" filters
-- - SEQUENCE comparison on (uid) for state-machine reconciliation
CREATE INDEX IF NOT EXISTS idx_calendar_events_organizer
    ON calendar_events(organizer)
    WHERE organizer IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_calendar_events_active_dtstart
    ON calendar_events(calendar_id, dtstart)
    WHERE status IS DISTINCT FROM 'CANCELLED' AND dtstart IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_calendar_events_uid_seq
    ON calendar_events(uid, sequence DESC);

-- Backfill is intentionally not done here. Existing rows carry summary +
-- dtstart + dtend already (extracted at migration-023 time), so basic
-- conflict queries still work with NULL organizer / empty attendees /
-- sequence=0. The CalDAV PUT handler in MRS-3 fills the new columns going
-- forward; old rows will pick up real values the next time the client
-- syncs them (CalDAV clients re-PUT modified events).
