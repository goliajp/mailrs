-- migration 033 — support per-instance overrides via RECURRENCE-ID.
--
-- Until now the UNIQUE constraint on calendar_events was
-- (calendar_id, uid). That collapsed every occurrence of a recurring
-- series into a single row, which is fine for "accept the whole series"
-- but blocks the per-instance RSVP path RFC 5545 §3.8.4.4 / RFC 5546
-- §3.4 require: an organizer can re-issue a single occurrence with a
-- new DTSTART, the attendee can RSVP that occurrence independently of
-- the master series, etc.
--
-- New shape:
-- - master series row keeps `recurrence_id IS NULL`, uniqueness via
--   partial index over (calendar_id, uid) WHERE recurrence_id IS NULL
-- - per-instance override has the original master's UID + a non-NULL
--   recurrence_id pointing at the occurrence start; uniqueness via
--   partial index over (calendar_id, uid, recurrence_id) WHERE NOT NULL
--
-- PostgreSQL 14+ supports partial-index conflict targets, which is what
-- the upsert callers (calendar::event::upsert_from_parsed_invite,
-- web::dav::event_put fallback path) now reference.

ALTER TABLE calendar_events
    DROP CONSTRAINT IF EXISTS calendar_events_calendar_id_uid_key;

CREATE UNIQUE INDEX IF NOT EXISTS uq_calendar_events_master
    ON calendar_events(calendar_id, uid)
    WHERE recurrence_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_calendar_events_instance
    ON calendar_events(calendar_id, uid, recurrence_id)
    WHERE recurrence_id IS NOT NULL;

-- The active-dtstart partial index from migration 031 still applies; no
-- change needed.
