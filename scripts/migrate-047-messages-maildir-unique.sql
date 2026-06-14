-- migrate-047: (mailbox_id, maildir_id) uniqueness on messages.
--
-- Groundwork for the receiver-decouple notification + reconcile design
-- (.claude/plans/20260614-mailrs-receiver-exec.html, step S1.1). The core
-- will discover newly-delivered mail via a pub/sub notification and fall
-- back to a low-frequency reconcile sweep — both of which can ask to index
-- the SAME delivered maildir file more than once (a re-fired notification,
-- or reconcile racing the live delivery). index_message now checks for an
-- existing row keyed by (mailbox_id, maildir_id) and returns its uid
-- instead of reserving a new one; this index is the DB-level backstop that
-- turns any double-index that slips past the check into a loud error
-- rather than a silent duplicate row / UID gap.
--
-- Pre-flight (run before applying; must return zero rows — existing dups
-- would make the CREATE fail, which is the intended fail-loud behavior):
--
--   SELECT mailbox_id, maildir_id, count(*)
--   FROM messages GROUP BY mailbox_id, maildir_id HAVING count(*) > 1;
--
-- Down: DROP INDEX IF EXISTS messages_mailbox_maildir_uniq;

CREATE UNIQUE INDEX IF NOT EXISTS messages_mailbox_maildir_uniq
    ON messages (mailbox_id, maildir_id);
