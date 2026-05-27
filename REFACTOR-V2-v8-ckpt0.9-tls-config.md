# v8 ckpt 0.9 — STARTTLS-success + Require-policy coverage

Closes the `worker/smtp.rs ≥ 80 %` trigger that ckpt 0 carved out.
Pure follow-up — no new fixtures, no production semantics change,
just one new public API hook in `mailrs-smtp-client` plus the
worker plumbing to thread it.

## What got built

### Production API change (non-breaking)

`mailrs-smtp-client::connection`:

- New `pub async fn SmtpConnection::try_starttls_with_config(self, hostname, client_config: ClientConfig) -> StarttlsResult`
- Existing `try_starttls` is now a thin wrapper: `try_starttls_with_config(self, hostname, default_pkix_client_config())`
- New `pub fn default_pkix_client_config() -> ClientConfig` — the
  `webpki-roots` PKIX config that production always uses
- `default_pkix_client_config` re-exported from crate root

Why: production paths keep using `try_starttls` (unchanged
behavior). Integration tests and downstream stones that need a
non-PKIX trust path (DANE-only, pinned-cert, skip-verify for
mock servers) inject their own `ClientConfig` via
`try_starttls_with_config`.

`mailrs-outbound-queue::worker::smtp`:

- `try_deliver_via_mx_with_tls` is now `pub` with new
  `tls_config_override: Option<Arc<rustls::ClientConfig>>` parameter
- `try_deliver_via_mx` (production entry) calls
  `try_deliver_via_mx_with_tls(.., TlsPolicy::Opportunistic, None, ..)`
  unchanged
- `worker/mod.rs` re-exports `try_deliver_via_mx_with_tls` for
  integration tests
- Adds `rustls = "0.23"` (default-features = false) to outbound-queue
  prod deps because the public API signature now mentions `ClientConfig`

Why: the worker's STARTTLS-success branch (lines 86–97 in
`worker/smtp.rs`) was uncoverable without injecting a skip-verify
verifier — the mock SMTP server presents a self-signed cert,
which `webpki-roots` correctly refuses. The override is a thin
test-injection seam: production code paths always pass `None` and
get the default PKIX config back.

### New integration tests (4)

`outbound-queue/tests/worker_delivery_integration.rs`:

| Test | Coverage |
|---|---|
| `try_deliver_via_mx_starttls_success_full_deliver` | STARTTLS handshake → re-EHLO over TLS → MAIL/RCPT/DATA → 250 OK. Drives `StarttlsResult::Success` branch (lines 86–97). |
| `try_deliver_via_mx_require_policy_rejected_starttls_returns_err` | TLS Require + 502 STARTTLS → Err. Drives `StarttlsResult::Rejected` + `tls_policy == Require` branch (lines 98–105). |
| `try_deliver_via_mx_require_policy_handshake_fail_returns_err` | TLS Require + server closes after `220 Ready` → Err. Drives `StarttlsResult::HandshakeFailed` + Require branch (lines 119–124). |
| `try_deliver_via_mx_require_policy_no_starttls_returns_err` | TLS Require + EHLO doesn't advertise STARTTLS → Err. Drives the `!ehlo_resp.has_extension("STARTTLS")` + Require branch (lines 150–157). |

### New `tests/common/mock_smtp.rs` helper

`pub fn skip_verify_client_config() -> rustls::ClientConfig` —
dangerous (no-validation) client config for STARTTLS-success
integration tests. ONLY safe inside test binaries.

## Coverage delta (vs ckpt 0)

| Module | ckpt 0 | ckpt 0.9 | Target | |
|---|---:|---:|---:|---|
| `outbound-queue/worker/smtp.rs` | 66.04 % | **83.83 %** | ≥ 80 % | ✓ |
| outbound-queue total lib | 90.01 % | **90.84 %** | | ↑ |

All other ckpt 0 numbers unchanged (no test files modified outside
worker_delivery_integration.rs).

**All 7 v8 trigger lines now met.**

## What's still NOT covered

`worker/smtp.rs` has 27 missed regions out of 167 (16.17%) post-ckpt-0.9:

- DANE-required handshake-fail / rejection branches (lines 109–117,
  130–138). Driving these requires a mock hickory resolver that
  synthesises TLSA records — not in scope for this ckpt. The
  Opportunistic-policy + has_dane path is also DNS-driven and
  similarly out of reach without a resolver mock.
- A handful of error-path edge lines that need specific I/O fault
  injection (timeout mid-EHLO under TLS, etc.) — diminishing returns
  vs the work to drive them deterministically.

If we ever need to push `worker/smtp.rs` past ~85 %, the next move
is a hickory mock resolver. Not blocking ckpt 1.

## Dev-deps / prod-deps changes

```
outbound-queue/Cargo.toml [dependencies]
+ rustls = { version = "0.23", default-features = false }
  # type-only — production passes None to try_deliver_via_mx_with_tls
```

No new dev-deps (skip-verify config helper lives in existing
tests/common/mock_smtp.rs).
