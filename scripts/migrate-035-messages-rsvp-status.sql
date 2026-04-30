-- migration 035 — persist user's RSVP decision on the invite message (MRS-19).
--
-- Bug: clicking Accept / Tentative / Decline only updated the user's own
-- calendar_events row + sent the iTIP REPLY out. The reply state was kept
-- in React local state and lost on page refresh — the invite-card snapped
-- back to fresh-buttons mode after every reload, even though the
-- organizer-side reply was already in flight.
--
-- Fix: snapshot the partstat onto the message row at RSVP time so the
-- web client can render an "already replied" state on subsequent loads.
-- Source of truth for organizer-side delivery is still the outbound queue;
-- this column just covers UI persistence.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS rsvp_status TEXT,
    ADD COLUMN IF NOT EXISTS rsvp_at TIMESTAMPTZ;

-- Constrain at the application layer (ACCEPTED / TENTATIVE / DECLINED) so a
-- future partstat (DELEGATED, COUNTER acceptance) doesn't require a
-- migration to extend an enum.
