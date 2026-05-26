# v6 ckpt 4 — Security walk + metrics gap closure (2026-05-27)

> Re-walk of the v0.6 OWASP top-10 (commit `6577632`) after the v6
> ckpt 1-3 churn (god-file split, mail-auth → mailrs-*, dkim h= fix,
> perf bench measurement). Plus the ckpt 4 metric-gap audit.

## OWASP top-10 — 10/10 ✅

Updated relative to `REFACTOR-V2-v0.6-security.md`:

| # | Category | Status | Evidence |
|---|---|---|---|
| A01 | Broken Access Control | ✅ ok | `permission.rs` RBAC, `require_permission()` on every admin endpoint; **no change from v0.6**. |
| A02 | Cryptographic Failures | ✅ ok | rustls 1.x for TLS, argon2 for password, HMAC-SHA256 for webhook sig, RFC 6376 for DKIM (now via `mailrs-dkim` instead of `mail-auth`, same crypto primitives). |
| A03 | Injection | ✅ ok | sqlx parameterized queries everywhere; new `count_pending` / `count_inflight` queue helpers use bind not format!. |
| A04 | Insecure Design (plaintext IMAP/POP3) | ✅ ok (was ⚠️ partial) | Plaintext listeners log `event="plaintext_listener_active"` WARN on startup + accept `MAILRS_DISABLE_PLAIN_IMAP=1` / `MAILRS_DISABLE_PLAIN_POP3=1` env opt-out (`crates/server/src/bootstrap/listeners.rs:91-200`). The threat model is "legacy clients need STARTTLS upgrade on port 143/110"; defaulting off would break those, so we surface the warning loudly and let operators opt out per RFC 1939 / RFC 3501. Promoted from partial → ok because the env knob + warn covers the realistic ops path. |
| A05 | Security Misconfiguration | ✅ ok | TLS cert validation default, rate-limit + auth-guard built-in, security headers middleware. |
| A06 | Vulnerable Components | ✅ ok | `scripts/check-security.sh` (`cargo audit && cargo deny check`) — re-run 2026-05-27 still clean (0 unhandled advisories; 1 documented ignore for RUSTSEC-2023-0071 — Marvin Attack on `rsa`, see v0.6 threat model). Workspace dep count post-mailrs-* cutover: 695 transitive deps, same advisory profile. |
| A07 | Auth Failures | ✅ ok | argon2 + per-IP lockout (`mailrs-auth-guard`) + TOTP; `mailrs_auth_total{outcome=success/failure}` counter. |
| A08 | Data Integrity Failures | ✅ ok | `deny.toml allow-git = []`; only crates.io registry; no curl|sh installer; `git push --tags` gated through `release.sh`. |
| A09 | Logging Failures | ✅ ok | Structured `event=<name>` tracing on all hot paths; webhook signature + DKIM reject events recorded; new metric gauges/histograms scrapable at `/metrics`. |
| A10 | Server-Side Request Forgery | ✅ ok (was ⚠️ partial) | `crates/server/src/web/mail/proxy.rs:13-50,86-95,171` — `is_safe_proxy_url()` rejects loopback (127/8, ::1, "localhost"), private IPv4 (10/8, 172.16/12, 192.168/16), link-local (169.254/16), and unique-local IPv6 (fc00::/7). Both `/api/proxy/image` and `/api/proxy/link` gate through it. Unit tests cover the rejection paths. Promoted from partial → ok. |

## Metrics gap closure

The ckpt 4 plan called for "outbound delivery latency / queue depth /
per-stage timing". Audited current metrics — 8 names existed
(`mailrs_auth_total`, `mailrs_connections_total/_active`,
`mailrs_imap_connections_total`, `mailrs_inbound_verdict_total`,
`mailrs_mcp_sessions_total`, `mailrs_messages_total`,
`mailrs_pop3_connections_total`). Added 3 missing key names:

| Metric | Type | Labels | Site |
|---|---|---|---|
| `mailrs_smtp_connections_total` | counter | `tls=plain\|implicit` | `smtp_session/mod.rs:80,238` — was missing entirely (only IMAP/POP3 were tracked). |
| `mailrs_outbound_queue_depth` | gauge | `status=pending\|inflight` | `outbound-queue/worker/mod.rs:191-201` — sampled every poll tick from PG `count(*) WHERE status=...`. |
| `mailrs_outbound_delivery_seconds` | histogram | `outcome=delivered` | `outbound-queue/worker/delivery.rs:158-165` — wall-clock from batch-claim to per-message `mark_delivered`. (Failed/bounced paths logged as backlog — needs same per-mark instrumentation.) |

Per-stage timing for the inbound pipeline (PtrStage / MailAuthStage /
ClamavStage / ContentScanStage / AiScoringStage individually) is the
next gap — punted to v7 ckpt-4-followup because each stage's
`Stage::evaluate` would need wrapping, and the `mailrs-inbound`
crate doesn't yet have a per-stage timing facade.

## What's NOT in this walk

- **TLS-RPT / MTA-STS / ARC dashboard JSON** — ckpt 4 plan mentioned
  designing Grafana panels. Punted: prod stack (`devops.golia.jp`)
  doesn't currently have a Grafana instance pointing at mailrs's
  `/metrics`. Listed as a v7 followup once that infra is up.
- **Sieve AGPL rewrite** — already a known DEPS_AUDIT candidate #4;
  out of v6 scope.
- **`rsa` crate Marvin Attack residual risk** — same threat model as
  v0.6: mailrs doesn't expose per-op RSA timing oracles.

## Trigger satisfied

- ✅ OWASP top-10 walk table covers 10/10.
- ✅ `cargo audit` + `cargo deny check` clean (re-run 2026-05-27).
- ✅ 3 missing key metric names added (smtp connections / queue depth
  / delivery latency); 11 names total now exposed via `/metrics`.

→ proceed to ckpt 5 (测试覆盖) when ready.
