-- migrate-049: index outbound_queue for the "have I written to this
-- address?" lookup that importance scoring performs.
--
-- `has_sent_to` reads the delivered history straight out of
-- outbound_queue rather than keeping a denormalised sent_count, because
-- the queue row already is the fact and only 'pending' rows are ever
-- deleted. That predicate is (sender, recipient), which neither
-- existing index serves:
--
--   idx_queue_pending  (status, next_retry)
--   idx_queue_domain   (domain) WHERE status = 'pending'
--
-- Without this index the lookup is a seq scan over a table that stores
-- full message bodies, once per inbound message — the exact shape of
-- the 2026-07-19 incident (see rules/hot-path-needs-a-plan.md, where a
-- 48k-row table served 309 billion rows because a composite index's
-- leading column was not supplied).
--
-- Verify after applying, on a database with real data:
--   EXPLAIN (ANALYZE, BUFFERS)
--   SELECT EXISTS (SELECT 1 FROM outbound_queue
--                  WHERE sender = 'a@b.c' AND recipient = 'd@e.f'
--                    AND status = 'delivered' LIMIT 1);
-- Expect an Index Scan, not "Rows Removed by Filter: <table size>".
--
-- Partial on status='delivered': the question is only ever asked about
-- delivered mail, and it keeps the index off the pending churn that
-- the queue worker rewrites constantly.

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_outbound_sender_recipient
    ON outbound_queue (sender, recipient)
    WHERE status = 'delivered';
