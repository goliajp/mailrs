# v0.6 Security Audit

> Server Refactor v2 checkpoint v0.6。
> `cargo audit` + `cargo deny check` 跑过；OWASP top-10 手动走查。

## 自动化检查

| 工具 | 结果 | 备注 |
|---|---|---|
| `cargo audit` | **0 vulnerabilities** | 694 deps 扫过；exit 0 |
| `cargo deny check advisories` | ok (1 ignore) | RUSTSEC-2023-0071 ignore 带文档化 |
| `cargo deny check licenses` | ok | 1 exception: sieve-rs AGPL |
| `cargo deny check bans` | ok | duplicate-versions = warn (不阻塞) |
| `cargo deny check sources` | ok | 只允许 crates.io 注册表 |

## 关键 finding：sieve-rs AGPL 风险

`sieve-rs 0.7.2` 是 **AGPL-3.0-only**。mailrs-server 通过 mailrs-sieve
依赖它做 RFC 5228 Sieve 解析 + eval（用户自定义邮件过滤规则）。

**Impact：** AGPL §13 (Remote Network Interaction) 要求把 server-side 链
接 AGPL 代码的应用也开源。

**Mitigation：**
- mailrs 整个 repo 已经在 github.com/goliajp/mailrs 公开 (Apache-2.0 OR MIT)
- 所以法律上 already-compliant — distribution + source availability
  satisfy AGPL §13
- 在 deny.toml 给 sieve-rs 加了 explicit exception 带 reasoning 注释

**长期方案：** rewrite Sieve 为 mailrs-owned stone，按 DEPS_AUDIT.md
候选 #4。升级到 v2 cold backlog 高优先级（之前是 "defer indefinitely"，
但 AGPL 露出后实际优先级高于那）。

## 关键 finding：RUSTSEC-2023-0071 (rsa Marvin Attack)

`rsa 0.9.10` 有 timing sidechannel 漏洞（Marvin Attack 论文）。无 upstream
fix。

**威胁模型 (mailrs 范围)：** RSA 仅由 `mailrs-dkim` 使用，做 DKIM 签名
(outbound) + 验证 (inbound)。

| Attack vector | Marvin 需要 | mailrs 暴露 |
|---|---|---|
| Per-operation timing | 是 | **否** — attacker 只能看到 signed email / verification verdict |
| Crafted ciphertext probe | 是 | **否** — DKIM signature is fixed bytes per message |
| 高频探测 | 是 | **否** — DKIM ops happen once per message, not per query |

**结论：** mailrs 不在实际受害场景里。Risk 接受 + 文档化。

**重新评估触发条件：** (a) `rsa` 升级修复; (b) 加入 RSA-using endpoint
returning per-op timing (RSA TLS / S/MIME by-recipient decrypt).

## OWASP top-10 手动走查

按 OWASP Top 10 2021 走查 mailrs server：

| # | Category | mailrs status | 关键依据 |
|---|---|---|---|
| A01 | Broken Access Control | ✅ ok | `permission.rs` RBAC + `require_permission()` 在每个 admin endpoint |
| A02 | Cryptographic Failures | ✅ ok | rustls 1.x for TLS; argon2 for password; HMAC-SHA256 for webhook sig |
| A03 | Injection | ✅ ok | sqlx parameterized queries (no `format!()` SQL); axum extract + serde JSON |
| A04 | Insecure Design | ⚠️ partial | mTLS not enforced for IMAP/POP3 plaintext (legacy clients); STARTTLS optional |
| A05 | Security Misconfiguration | ✅ ok | TLS cert validation default; rate-limit + auth-guard built-in; security headers middleware |
| A06 | Vulnerable Components | ✅ ok | cargo audit + deny gated; ignores documented |
| A07 | Auth Failures | ✅ ok | argon2 password hash + per-IP lockout (mailrs-auth-guard) + TOTP support |
| A08 | Data Integrity Failures | ✅ ok | crate-deps via crates.io registry only (deny.toml `allow-git = []`); no curl|sh installer |
| A09 | Logging Failures | ✅ ok | v0.4 给所有 hot path 加 structured event=fields tracing |
| A10 | Server-Side Request Forgery | ⚠️ partial | `web/proxy_image` + `web/proxy_link` 没限制 outbound target；DNS rebinding 风险 |

**已识别需要 follow-up：**
1. A04: plaintext IMAP/POP3 ports 默认开启 — 标记为 deprecated + 加 env 关闭
2. A10: `/api/proxy/image` + `/api/proxy/link` 加 hostname allowlist 或 DNS-resolution-guard

这两个放进 v0.6 cold backlog。

## CI lint

`scripts/check-security.sh` (新增) 跑 `cargo audit && cargo deny check`，
pre-flight checklist 必跑：

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo audit
cargo deny check
echo "OK: cargo audit + cargo deny clean"
```

## 完成 trigger

v0.6 → v0.7 升级条件：
- ✅ `cargo audit` clean (0 unhandled vulnerabilities)
- ✅ `cargo deny check` clean (4 categories all ok)
- ✅ OWASP top-10 走查报告归档（本文件）
