# Deploy + Rollback (v0.7)

> Server Refactor v2 v0.7 added a deploy health gate to `scripts/deploy.sh`.
> Old binary auto-backed-up; new binary auto-rolled-back on health failure.

## Flow

```
1. pre-deploy health check (ssh + curl /api/health on prod)
     │
     ├─ healthy / degraded → continue
     ├─ unhealthy          → abort (override with FORCE_DEPLOY=1)
     └─ unreachable        → assume first-deploy, continue
2. backup current binary → /root/backup/mailrs-server.<timestamp>
3. cross-compile new binary (cargo zigbuild)
4. upload binary + web assets + configs + .env + migrations
5. docker compose build --no-cache && up -d
6. post-deploy health check: poll /api/health every 2s up to 60s
     │
     ├─ healthy / degraded → done, print container logs, exit 0
     └─ unhealthy / timeout → rollback path
                                ├─ restore backup binary
                                ├─ docker compose up -d --force-recreate
                                ├─ re-check health
                                ├─ rollback ok → exit 1 (release tag aborted)
                                └─ rollback failed → exit 1 + scream
```

## Health gate

Endpoint: `http://localhost:3200/api/health` (server-internal, not via TLS).

Expected response shape:

```json
{
  "status": "healthy" | "degraded" | "unhealthy",
  "level": 0 | 1 | 2 | 3,
  "pg": true | false,
  "valkey": true | false,
  "uptime_secs": N,
  "version": "X.Y.Z",
  ...
}
```

Deploy treats `healthy` + `degraded` as ok (degraded usually means PG /
Valkey transiently unavailable but server still serves auth + read).

## Environment knobs

| Var | Default | Use |
|---|---|---|
| `SSH_KEY` | `~/keys/aws.pem` | SSH private key |
| `SSH_HOST` | `root@t02.golia.jp` | host:user |
| `HEALTH_URL` | `http://localhost:3200/api/health` | curled from inside SSH |
| `HEALTH_TIMEOUT_SECS` | 60 | post-deploy poll deadline |
| `FORCE_DEPLOY` | unset | set to `1` to deploy onto already-unhealthy prod |

## Rollback details

- Backups live in `/root/backup/mailrs-server.<YYYYMMDD-HHMMSS>` on host.
- Rollback restores `bin/mailrs-server` from the most recent backup
  (the one created at the start of the current deploy run).
- Backups are never auto-deleted — clean periodically:
  `ssh $SSH_HOST 'find /root/backup -name "mailrs-server.*" -mtime +30 -delete'`

## release.sh integration

`scripts/release.sh` calls `deploy.sh` inside a trap. If deploy.sh
exits non-zero (which now includes post-deploy health failure or
rollback completion), release.sh rolls back the local version bump
(Cargo.toml + Cargo.lock + web/package.json) — so the working tree
stays clean and the next attempt starts from the same base.

## Testing the gate

Without an actual broken build, you can dry-run by setting the health
URL to a non-existent endpoint:

```bash
HEALTH_URL=http://localhost:9999/nope ./scripts/deploy.sh
```

Expected: pre-deploy check shows `unreachable`, build + upload
succeed, post-deploy hits TIMEOUT, rollback fires.

## Manual rollback (if auto-rollback failed)

```bash
ssh root@t02.golia.jp '
  cd /apps/mailrs &&
  ls -t /root/backup/mailrs-server.* | head -1 |
    xargs -I {} cp {} bin/mailrs-server &&
  docker compose up -d --force-recreate
'
```
