# Dual-mode core switch runbook (v2)

The serving core is switchable between **fastcore** (kevy-backed, default)
and **core** (pg/spg-backed). `split` (receiver → spool → core) is
permanent; only the core behind `MAILRS_CORE_RPC_BASE` changes. webapi /
sender are 100% agnostic — the switch is one env var + which core
container runs.

## Topology

| mode | core container | `MAILRS_CORE_RPC_BASE` | compose |
|---|---|---|---|
| fastcore (default) | `mailrs-fastcore` (`mailrs-fastcore`, :3301) | `http://mailrs-fastcore:3301` | `docker-compose.prod.yml` |
| core (pg/spg) | `mailrs` (`mailrs-server --features core-rpc[,spg]`, :3300) | `http://mailrs:3300` | `docker-compose.split.yml` |

Shared, unaffected by the switch: network kevy (sessions/greylist/sieve/
contacts/queue/…) and the maildir on disk. Only the mail store (threads/
messages/uids/mailboxes/accounts/aliases) lives in the switchable core.

## Switch (either direction — the sync tool is direction-blind)

```bash
# 1. bring the NEW core up alongside the old (source stays readable)
#    e.g. switching fastcore -> pg-core: start the `mailrs` + postgres
#    services from docker-compose.split.yml.

# 2a. LOOK FIRST. Reads both sides, writes nothing.
MAILRS_CORE_API_SECRET=<secret> \
  mailrs-core-sync --from http://<old-core> --to http://<new-core> --dry-run
#   Read the second line of its output, not the first. "threads examined"
#   cannot come out zero and so measures nothing; "already match on both
#   sides" can. A large difference on a first run is usually a backfill gap
#   rather than a defect — this repo's last such migration reported 19,779
#   differing rows, which converged to 74 after two backfills, and cutting
#   over on the first figure would have shipped a worse fault than the one
#   being fixed.
#   Also read "accounts only on the destination": a copy forward will not
#   remove them, so after the switch their mail is readable on one core and
#   not the other.
#   If it REFUSES with "more than 10000 threads share second N", raise
#   --page-size. It refuses rather than reporting a lower count because a
#   warning that some conversations were not enumerated is a warning that
#   some mail did not come across, and you are about to flip a switch on it.

# 2b. migrate the mail store over the contract (one-shot, idempotent)
MAILRS_CORE_API_SECRET=<secret> \
  mailrs-core-sync --from http://<old-core> --to http://<new-core>
#   fastcore->pg:  --from http://mailrs-fastcore:3301 --to http://mailrs:3300
#   pg->fastcore:  --from http://mailrs:3300 --to http://mailrs-fastcore:3301

# 3. flip the switch + restart the public entry
#    set MAILRS_CORE_RPC_BASE=<new-core> in .env
docker compose up -d webapi-fc

# 4. verify, then retire the old core container.
#    No search index to rebuild: each core owns its own. kevy maintains
#    its text index from the commit hook as core-sync writes the rows;
#    pg maintains search_vector by trigger on insert.
```

## Rollback

Re-run `mailrs-core-sync` in the reverse direction and flip
`MAILRS_CORE_RPC_BASE` back. Idempotent — the sync's per-thread
message-id dedup means re-running never double-inserts.

**Rehearse it before you need it.** Not as caution for its own sake: the
reverse direction exercises the *other* core's enumeration read path, which
the forward direction never touches, and a rollback is the one operation
nobody wants to be performing for the first time. Do the whole loop on the
staging pair — switch, run the cross-lane suite, switch back, run it again —
and treat the same differences appearing both times as the evidence that the
write path did not pollute anything on the way through. Differences that
appear only on the second pass are new damage.

The cross-lane suite is the check:

```bash
cargo test -p mailrs-server --features core-rpc,spg two_lane
```

Tolerated differences and their reasons are in
`.claude/two-lane-known-diff.txt`. Anything not on that list is a
regression, and the list is held honest by a test that fails when an
exclusion has stopped being necessary.

## Notes

- spg is no longer held. The two bugs that blocked it — connection-pool
  exhaustion and a crash-recovery deadlock — closed in 7.37.8 and
  7.37.11/12, and the workspace is on 7.37.16. Either backend is a
  build-flag choice under the same `PgMailboxStore` (`--features core-rpc`
  for real PostgreSQL, `--features core-rpc,spg` for spg-embedded); the
  contract and this runbook are the same for both. Both are built and both
  are gated.
- uid identity is NOT preserved cross-backend (each core allocates its
  own); only per-mailbox monotonicity holds. IMAP clients re-sync uids
  after a switch — expected.
