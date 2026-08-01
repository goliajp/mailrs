# Deploy + Rollback

Prod is the 4-process fastcore stack on `t02.golia.jp`.

**The default deploy is local, not CI:**

```bash
./scripts/direct-deploy.sh <X.Y.Z>
```

It runs the whole gate (mcp + rest parity, `cargo fmt --check`, `clippy
-D warnings`, the full suite `--no-fail-fast`, `perf-gates.sh`), builds
arm64, ships the image and the compose file to t02, rolls the four roles,
then verifies: `/v1/healthz` 200, the version on `:3103`, a 27-route probe,
and the kevy AOF replay line reading `(clean)`. Finally it builds and
rsyncs the web bundle.

Nothing on the develop path runs in CI, so that gate is the only thing
between a change and production. `SKIP_GATE=1` means shipping unverified
code, and the script says so.

**CI release** — for a multi-arch image and a GitHub Release:
`./scripts/release-tag.sh v<X.Y.Z>`. Same binary, built more formally on
self-hosted runners. It has **no staging gate**; that was removed on
2026-07-21.

**Web-only releases** go through the parallel `web-v*` lane
(`release-web.yml`, rsync into a bind mount, no container restart) —
`direct-deploy.sh` already ships web, so this is for the tagged path.

## Prod topology (since 2026-07-03)

```
mail.golia.ai  →  nginx (t02)
                       ├─ receiver          :25 / :465 / :587    SMTP inbound
                       ├─ webapi-fc         :3103 → nginx TLS    REST + web UI + MCP
                       ├─ fastcore          :3201 core-api RPC,
                       │                    :143 / :993 IMAP,
                       │                    :110 / :995 POP3,
                       │                    spool drain / sieve /
                       │                    self-heal / live sync
                       └─ fastcore-sender   outbound delivery,
                                            DKIM signer

                       ─ kevy               RESP :6379 (shared)
                       ─ meilisearch        :7700
                       ─ chromium           HTML→PDF renderer
```

Canonical compose: `deploy/docker-compose.prod.yml`. On every `v*`
release release.yml scp's it to `/apps/mailrs/docker-compose.yml`
and runs `docker compose up -d --remove-orphans`.

## Cutting a release

```bash
# from develop, after any feature branches have landed
git flow release start v<X.Y.Z>
GIT_MERGE_AUTOEDIT=no git flow release finish -p -m "Release v<X.Y.Z>" v<X.Y.Z>
```

No version bump. Cargo.toml + web/package.json stay pinned at
`0.0.0`; release.yml `sed`s the real version into both from
`${GITHUB_REF_NAME#v}` before building.

The tag push triggers release.yml. Watch it:

```bash
gh run watch $(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
```

Prod is healthy when all four fastcore roles report the new tag
and `curl https://mail.golia.ai/api/health` returns 200.

## Staging (retired from the release path, 2026-07-21)

There is no soak gate. release.yml had one — sha match against
`/etc/staging-deploy-sha` on t01, `.pass == true` in
`/var/run/staging-gate.json`, verdict younger than an hour — and it was
removed because t01 is shared with other projects: a busy neighbour
starved staging's accept loop into 16–80 second requests and the gate
failed releases on that noise while `slow_pct` sat at ~0%.

`scripts/staging-build-deploy.sh` still works for ad-hoc use and the
soak units on t01 are stopped and disabled. Nothing in the release path
consults either.

## Rollback runbook

release.yml has no built-in auto-rollback. If a `v<X.Y.Z>` deploy
misbehaves in prod, the recovery is manual:

1. **Confirm the failure mode.** `ssh t02 docker logs mailrs-fastcore
   --tail 200` — look for panics, kevy replay errors, or spg-lane
   errors depending on the surface hit.
2. **Roll the image tag back.** On t02:
   ```bash
   cd /apps/mailrs
   sed -i.bak 's/:<X.Y.Z>/:<X.Y.Z-1>/g' docker-compose.yml
   docker compose up -d --remove-orphans
   ```
   The compose file references the image as
   `ghcr.io/goliajp/mailrs:${MAILRS_VERSION}` where `MAILRS_VERSION`
   comes from `.env`. Edit that env, don't hand-edit compose.
3. **Verify all four roles are healthy** on the previous tag:
   `docker ps --format "{{.Names}} {{.Image}} {{.Status}}"` should
   list receiver + fastcore + webapi-fc + fastcore-sender all on
   the rolled-back version.
4. **kevy AOF recovery** if the failed deploy corrupted kevy state.
   The kevy container's `/data/kevy-fastcore/aof-*.aof` is the
   source of truth; stopping fastcore then restarting it replays
   the AOF. If the AOF itself is suspect, the tail may already have
   been quarantined by kevy 3.17 (look for
   `aof-*.aof.panic-quarantine.<unix_ts>`); dropping that leaves a
   clean replay.
5. **File an incident note** in `.claude/incidents/` with the
   failed tag, symptoms, revert path, and root-cause hypothesis.

For v2.0.0 specifically: the change-feed refactor (Stage B.8) sets
`Config::with_feed(16 MiB)` on the kevy Store. If a Store opened
with `with_feed` is downgraded to a binary that does not set
`with_feed`, the feed buffer is dropped — a subsequent upgrade back
to v2.0.0 starts the feed from a fresh generation, so IMAP IDLE
consumers must call `changes_tail()` and resume from the returned
cursor (already the code path in `imap/session.rs`).

## Web release

```bash
TAG="web-v$(date +%Y.%m.%d)-1"
git flow release start "$TAG"
GIT_MERGE_AUTOEDIT=no git flow release finish -p -m "Web release" "$TAG"
```

release-web.yml runs `bun run build` and rsync's `dist/` into
`/apps/mailrs/web/` on t02. The container isn't touched — its
`ServeDir` reads from the bind mount on every request, so a
rsync-in takes effect immediately. No downtime.

## Common recovery scenarios

- **Fastcore won't start / kevy replay hangs.** Check
  `/data/kevy-fastcore/aof-0.aof.panic-quarantine.*`; if a
  quarantined tail exists, replay picked a corrupt frame and
  isolated it. Recovery = boot fastcore normally, the AOF replays
  everything before the quarantine point.
- **webapi-fc healthy but MCP / conversation reads slow.** Look at
  the shared kevy container's memory / AOF size. `docker exec
  mailrs-kevy kevy-cli info` reports live keys + used_memory +
  aof_bytes. A full AOF rewrite is auto-triggered at 100 % growth /
  64 MiB (kevy default); an unattended one can be forced with
  `docker exec mailrs-kevy kevy-cli rewrite_aof`.

## What's monitored

Prod has no external synthetic monitor — the t02 memory watchdog is the
only alarm surface. If prod goes unresponsive, first check
`curl https://mail.golia.ai/api/health`; if that hangs, `ssh t02 docker
ps` to see whether the containers are up.

The post-deploy checks live in `direct-deploy.sh` (and, for the tagged
path, in release.yml): `/v1/healthz`, the version on `:3103`,
`scripts/post-deploy-probe.sh`'s 27-route table, and the AOF replay
line. Each of those exists because a deploy once passed without it —
the version probe printed and continued while the REST API was down,
and the fastcore probe asked for a `/healthz` that answers 404 and
accepted any non-000 code.
