-- migrate-050: the two columns the SQL lane is actually missing.
--
-- Sizing note, because the plan for this migration was much larger. It
-- budgeted two new tables — `threads` and `thread_users` — on the strength of
-- a line in .claude/rfcs/20260730-per-user-thread-state.md saying the PG lane
-- still had the multi-owner defect the kevy side had been fixed for. It does
-- not: crates/mailbox/tests/multi_owner.rs proves that on both backend axes.
-- `messages` is one row per mailbox and a mailbox belongs to one account, so a
-- thread two accounts received has two disjoint sets of rows, and
-- `list_conversations` filters `mb.user_address` before aggregating. Every
-- per-user field already has a home: archived and pinned as columns, starred
-- as the IMAP \Flagged bit, unread as \Seen, snooze keyed by
-- (thread_id, account_address). kevy's membership row is a denormalisation
-- patch for a shared-hash model this side does not have.
--
-- What is genuinely absent is these two.

-- 1. messages.sender_trust
--
-- The verdict `mailrs_inbound::sender_trust` folds out of a message's own
-- Authentication-Results at ingest: 'verified' (DMARC pass), 'suspicious' (an
-- auth method failed — likely spoofed), 'unverified' (auth ran, nothing
-- conclusive), or '' for mail ingested before the field existed. It is on
-- MessageWire and the kevy lane writes it; this lane had nowhere to put it, so
-- the badge the client renders would read empty for every message.
--
-- No index: it is read with the row and never filtered on. A column nobody
-- searches by does not need one, and an index on a four-value string over the
-- largest table here would cost writes for nothing.
ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS sender_trust TEXT NOT NULL DEFAULT '';

-- 2. outbound_queue.scheduled_at
--
-- Epoch seconds, and NULL means "send now" — matching `scheduled_at:
-- Option<i64>` on the wire and this table's existing BIGINT convention
-- (next_retry, created_at, updated_at are all epoch BIGINT, not TIMESTAMPTZ).
ALTER TABLE outbound_queue
    ADD COLUMN IF NOT EXISTS scheduled_at BIGINT;

-- The due sweep asks "what is ready to go out", which is a range over
-- scheduled_at:
--
--   WHERE scheduled_at IS NOT NULL AND scheduled_at <= <now>
--
-- Partial on NOT NULL, deliberately. Almost nothing is scheduled — the column
-- is NULL for every ordinary send — so a full index would be one entry per
-- queue row to serve a predicate that only ever concerns a handful, and it
-- would sit on the pending churn the queue worker rewrites constantly. The
-- partial index holds exactly the scheduled messages.
--
-- Verify after applying, on a database with real data:
--
--   EXPLAIN (ANALYZE, BUFFERS)
--   SELECT id FROM outbound_queue
--   WHERE scheduled_at IS NOT NULL AND scheduled_at <= 1755000000
--   ORDER BY scheduled_at LIMIT 100;
--
-- Expect an Index Scan. "Rows Removed by Filter: <table size>" means the
-- planner ignored it — and this table stores full message bodies, so a seq
-- scan here is the 2026-07-19 shape (rules/hot-path-needs-a-plan.md: a
-- 48k-row table served 309 billion rows because a composite index's leading
-- column was never supplied).
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_outbound_scheduled_at
    ON outbound_queue (scheduled_at)
    WHERE scheduled_at IS NOT NULL;
