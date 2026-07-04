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
contacts/queue/…), meili, maildir on disk. Only the mail store (threads/
messages/uids/mailboxes/accounts/aliases) lives in the switchable core.

## Switch (either direction — the sync tool is direction-blind)

```bash
# 1. bring the NEW core up alongside the old (source stays readable)
#    e.g. switching fastcore -> pg-core: start the `mailrs` + postgres
#    services from docker-compose.split.yml.

# 2. migrate the mail store over the contract (one-shot, idempotent)
MAILRS_CORE_API_SECRET=<secret> \
  mailrs-core-sync --from http://<old-core> --to http://<new-core>
#   fastcore->pg:  --from http://mailrs-fastcore:3301 --to http://mailrs:3300
#   pg->fastcore:  --from http://mailrs:3300 --to http://mailrs-fastcore:3301

# 3. flip the switch + restart the public entry
#    set MAILRS_CORE_RPC_BASE=<new-core> in .env
docker compose up -d webapi-fc

# 4. rebuild the derived meili index for the new core
docker run --rm --entrypoint mailrs-fastcore-backfill-meili ...   # (kevy dest)
#    (pg dest rebuilds its own FTS on insert; no meili backfill needed)

# 5. verify, then retire the old core container.
```

## Rollback

Re-run `mailrs-core-sync` in the reverse direction and flip
`MAILRS_CORE_RPC_BASE` back. Idempotent — the sync's per-thread
message-id dedup means re-running never double-inserts.

## Notes

- spg is currently held (conn-pool + crash-recovery bugs); pg-core uses
  plain PostgreSQL as the drop-in. When spg returns it is a build-flag
  flip (`--features core-rpc,spg`) under the same `PgMailboxStore` — the
  contract and this runbook are unchanged.
- uid identity is NOT preserved cross-backend (each core allocates its
  own); only per-mailbox monotonicity holds. IMAP clients re-sync uids
  after a switch — expected.
