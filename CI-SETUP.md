# CI/CD setup (v5 GitHub Actions migration)

This document captures the GitHub Actions + GHCR + docker-compose
deployment path introduced in v5 (mailrs ≥ v1.7.31). Old local
`scripts/release.sh` flow is preserved as fallback during the parallel-
run period — see [Cutover plan](#cutover-plan) for when to retire it.

## Surface

| File | Purpose |
|---|---|
| `.github/workflows/test.yml` | per-PR + per-push cargo test + bun test gate |
| `.github/workflows/release.yml` | `v*` tag push → cargo test → docker buildx → ghcr push → GH release |
| `Dockerfile` | unchanged (3-stage rust → web → debian:trixie-slim) |
| `docker-compose.yml` | dev compose, builds image locally |
| `docker-compose.prod.yml` | prod compose, pulls `ghcr.io/<org>/mailrs:<tag>` |
| `scripts/release.sh` | legacy local release (kept as fallback) |
| `scripts/deploy-from-ghcr.sh` | new remote-host script: pull + compose up |

## GitHub repo secrets needed

The release workflow pushes to GHCR using the built-in `GITHUB_TOKEN`,
so the only mandatory secret is the one GitHub gives you for free. The
following are **only** needed if/when you want CI to trigger remote
deployment (not part of v5.0–v5.2):

| Secret | When | Use |
|---|---|---|
| `GHCR_RO_TOKEN` | once any private downstream consumer pulls the image | Personal access token with `read:packages` for image consumers |
| `DEPLOY_HOST` | v5.3 | hostname:port of the deploy target (e.g. `t02.golia.jp:22`) |
| `DEPLOY_SSH_KEY` | v5.3 | private key authorized on the deploy host |
| `DEPLOY_WEBHOOK_URL` | v5.3 alt | webhook endpoint on deploy host that pulls + restarts |

No user-personal secrets are stored — every secret is a service
account / deploy bot scope.

## Cutover plan

Three checkpoints, each one explicit Go/No-Go.

### v5.0 → v5.1 — workflows land + dry-run

- [ ] `.github/workflows/test.yml` runs on every PR + push (green)
- [ ] `.github/workflows/release.yml` triggers on a dry-run tag
      (`vX.Y.Z-rc1`, gate via `prerelease: true`)
- [ ] Manual `docker pull ghcr.io/<org>/mailrs:<rc-tag>` works from a
      clean machine
- [ ] **No prod cutover yet.** `scripts/release.sh` keeps deploying.

### v5.1 → v5.2 — prod runs CI image side-by-side

- [ ] On the deploy host: clone repo, copy `.env` from current prod
- [ ] `MAILRS_VERSION=<rc-tag> docker compose -f docker-compose.prod.yml up -d` on a
      non-prod port (e.g. 3101 web, 2526 smtp) — shadow run
- [ ] Tail logs for ≥ 24h, confirm no divergence with current prod
- [ ] Compare `/api/health` + key endpoints between the two
- [ ] Still **no prod cutover.**

### v5.2 → v5.3 — flip prod to GHCR image

- [ ] Stop current prod via `scripts/release.sh stop` (or manual)
- [ ] `MAILRS_VERSION=<real-tag> docker compose -f docker-compose.prod.yml up -d`
- [ ] Health check passes, `/api/health` returns `ok`
- [ ] Send a few test mails through, verify Maildir + IMAP + web all good
- [ ] After 3 release cycles with zero rollbacks: retire
      `scripts/release.sh` (rename to `scripts/release-legacy.sh`,
      keep one cycle for archival, then delete)

## Anti-footguns

- **Never push to `main` from a workflow.** GitHub Actions has write
  permission via `GITHUB_TOKEN` but releases must originate from
  developer machines (where `scripts/bump.sh` mutates `Cargo.toml` and
  the commit signature carries developer identity).
- **No `--no-verify`** in any workflow git operation. If a pre-commit
  hook fails on CI, the answer is to fix the hook, not skip it.
- **Cache poisoning surface**: `actions/cache@v4` keyed on
  `hashFiles('**/Cargo.lock')` will share the cache between PR branches.
  This is fine — we only cache build artifacts, not anything secret.
- **Image tag mutability**: `latest` moves; `<version>` doesn't. Pin
  prod `.env` to a version tag, never to `latest`.

## When the v5 path is broken — rollback

Until the cutover is complete and 3 release cycles have passed, the
fallback is one command:

```bash
./scripts/release.sh patch    # local cargo + zigbuild + ssh, same as v1-v3
```

Document any CI breakage in the internal dev-history notes before
re-enabling. The legacy script depends on `SSH_KEY` + `SSH_HOST` env
vars — make sure those are still exported in your shell.
