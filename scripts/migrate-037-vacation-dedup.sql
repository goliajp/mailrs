-- Vacation auto-reply dedup state (RFC 5230 §4.6).
--
-- Tracks the last time a vacation auto-reply was sent for each
-- (recipient, original-sender, handle) triple, so a given sender
-- receives at most one reply per dedup window (:days / :seconds,
-- default 7 days). This is mutable dedup state, not an append-only
-- fact log — a row is upserted on each reply and may be pruned once
-- stale.
--
--   recipient = the vacationing mailbox owner
--   sender    = original envelope MAIL FROM
--   handle    = the :handle tag, or a value derived from
--               subject + reason by the caller when :handle is absent
--
-- The primary key (recipient, sender, handle) is also the lookup
-- index for "have we replied to this sender recently?".

CREATE TABLE IF NOT EXISTS vacation_dedup (
    recipient    TEXT NOT NULL,
    sender       TEXT NOT NULL,
    handle       TEXT NOT NULL,
    last_sent_at BIGINT NOT NULL,
    PRIMARY KEY (recipient, sender, handle)
);
