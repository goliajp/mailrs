# v0.2 Cement 二次审计

> Server Refactor v2 checkpoint v0.2。
> 这次重构刚把所有大文件拆完，是 module boundary 最清晰的时刻；
> 趁热打铁找漏网的 stone。

## 审计范围

`crates/server/src/` 下 40 个顶层 cement 文件 / module（按 prod LOC 排序）。

## 快速排除（明显 cement，不审）

| 类别 | 文件 | 理由 |
|---|---|---|
| Wiring / startup | `main.rs`, `bootstrap/`, `listeners.rs`, `tls.rs`, `acme.rs`, `pg.rs`, `valkey_store.rs` | 服务启动 wiring，零 generic 价值 |
| Session state machines | `imap_session/`, `smtp_session/`, `pop3_session/`, `managesieve_session.rs` | mailrs business semantics |
| PG schema-bound | `domain_store/`, `system_config.rs`, `users.rs`, `permission.rs`, `api_key_store.rs`, `oidc_store.rs`, `dmarc_report.rs`, `health.rs`, `conversation_cache.rs`, `reputation.rs`, `search_index.rs` | 直接读写 mailrs schema |
| RPC / API | `mcp/`, `web/`, `ldap_auth.rs`, `totp.rs` | mailrs-specific API surface |
| Wrappers around existing stones | `inbound/` (5 lines), `outbound_tls_rpt/` (41), `calendar/`, `webhook/` (61) | 已经是 stones 的薄 binding |
| Config | `config.rs` | MAILRS_* env vars |
| Background workers | `content_worker.rs`, `ai_analyzer.rs`, `rbl_monitor.rs` | Tokio task loops with mailrs business logic |
| Adapters | `event_bus.rs`, `oidc_jwt.rs` | mailrs SmtpEvent enum / mailrs JWT claims shape |
| Already audited & extracted | (history) | — |

## 候选清单（实际跑 lens）

### A. `fbl.rs` (37 LOC) → **抽取 `mailrs-arf`**

| Lens 问题 | 答 |
|---|---|
| non-mailrs 项目能用？ | ✅ 任何处理 abuse complaint 的 mail server（Mailgun / SendGrid / 自建 ESP）都需要 |
| 单句 identity？ | ✅ "RFC 5965 ARF feedback report parser" |
| 无项目特定 import？ | ✅ 只用 std |
| ≤500 LOC？ | ✅ 37 |
| 有 hot path 可 bench？ | ✅ parse text — 可以 bench |
| RFC / 算法边界？ | ✅ RFC 5965 (Abuse Reporting Format) |

**Crates.io 现状**：搜 `arf` / `feedback-report` / `rfc5965` 都没有专门的 Rust ARF parser。`arf` 0.1.0 是空 placeholder（"Initial claim for an on-going project"）。**`mailrs-arf` 是 first-mover。**

**社区价值高。** 单选 winner。

### B. `message_util.rs::extract_header_from_raw` → **dedup 而非新 stone**

这是 RFC 5322 header lookup 手写实现（28 行），重复了 `mailrs-rfc5322` 已有功能。
属于 v0.2 cold backlog 的 **dedup followup**：替换调用点使用 `mailrs_rfc5322`，
删除手写。不需要新 crate。

记入 cold backlog（不阻塞 v0.2 主路径）。

### C-Z. 其他候选

- `inline_image.rs` (197) — 解析 HTML 找 `<img src="cid:...">` + 写盘 maildir 的 inline image 文件 — 第二个动作 mailrs-specific，整体 cement
- `render_preview.rs` (409) — Chromium adapter + mailrs preview cache 绑定，cement
- `oidc_jwt.rs` (119) — mailrs 的 JWT issuer claims shape (audience = mailrs hostname, scope = mailrs-specific) — cement
- `rbl_monitor.rs` (130) — 用 `mailrs-dnsbl` stone 做 lookup，加了 5 个 well-known zone 列表 + 监控循环 — wiring，cement

**没有第二个明显 stone**。

## 决策

实施 A：抽出 `mailrs-arf` 1.0.0。

理由：
1. 边界最干净（37 行，RFC 标准）
2. 社区空白（crates.io 上 0 个 Rust ARF parser）
3. 单文件零依赖，抽出成本最低
4. 立刻可以补 bench + fuzz 走完 mailrs stone 标准流程

## 实施计划（v0.2 hot step 5-11）

1. 建 `crates/arf/` (mailrs-arf crate)
2. 移植 `fbl.rs` 的 `parse_arf_report` + 4 个测试
3. 加 `parse_report` 返回 `Report { recipient, feedback_type }` 结构（API 更现代）
4. 加 README / CHANGELOG / BUDGETS / criterion bench
5. 加 fuzz target（任意 byte input）
6. 在 server 加 `mailrs-arf = "1"` 依赖，替换 `fbl.rs` 调用
7. 删除 `crates/server/src/fbl.rs`
8. 更新 ARCHITECTURE.md / DEPS_AUDIT.md
9. `cargo publish` mailrs-arf 1.0.0
10. release.sh 发 server 版

## v0.2 Cold Backlog

- **dedup**: `message_util.rs::extract_header_from_raw` → 改用 `mailrs-rfc5322`
- **下一轮 cement 审计** 时再看 `webhook/` (delivery worker logic 还没全审)
