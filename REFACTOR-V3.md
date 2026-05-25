# Stones Polish + Backlog Sweep v3 — 4-Layer Plan

> v1 主线 (结构 / stones 抽出 / 性能基线 / 文档) ✅
> v2 server polish (性能/cement audit/metrics/logging/API drift/security/deploy) ✅
> **v3**: 41 个 published stones 全部经历 6 维度严苛审查 + v2 cold backlog 全部清空。

## L1 Roadmap (一句话)

每个 mailrs-* stone 都做到 **真实数据领先竞品 (rust/go/c/c++)** 或在
社区独占空白；6 维度（perf / mem / size / doc / test / bench）全过；
v2 遗留 cold backlog 12 项全清。

## L2 v3 边界 (定下不动)

### Part A — 41 stones × 6 维度审查

按当前完成度分 3 组（先磨刀，再啃硬骨头）：

**Tier A — "齐全 + fuzz"，12 个 stones** (起步组，搭审查流程)
- mailrs-arc, mailrs-arf, mailrs-dkim, mailrs-ical, mailrs-imap-proto,
  mailrs-mime, mailrs-mta-sts, mailrs-rfc2047, mailrs-rfc5322,
  mailrs-smtp-proto, mailrs-spf, mailrs-tls-rpt

**Tier B — "齐全缺 fuzz"，21 个 stones** (主体齐全，补 fuzz + 竞品)
- mailrs-acme, mailrs-auth-guard, mailrs-backoff, mailrs-clamav,
  mailrs-clean, mailrs-dav, mailrs-dmarc, mailrs-dnsbl, mailrs-inbound,
  mailrs-intelligence, mailrs-jmap, mailrs-mailbox, mailrs-maildir,
  mailrs-outbound-queue, mailrs-postmaster, mailrs-rate-limit,
  mailrs-rfc2231, mailrs-shield, mailrs-smtp-client, mailrs-srs,
  mailrs-tls-reload, mailrs-webhook-signature
- (注：mailrs-dns 也在这里 — 有 README+CHANGELOG+BUDGETS+bench 但缺
  perf_gate + fuzz)

**Tier C — "粗糙 / minimal"，7 个 stones** (从头补齐 6 维度)
- mailrs-attachment-extract (仅 README)
- mailrs-delivery-executor (仅 README)
- mailrs-imap-codec (仅 README)
- mailrs-imap-format (仅 README, **799 prod LOC**)
- mailrs-sieve (仅 README)
- mailrs-smtp-codec (仅 README)

### 每个 stone 的 6 维度审查清单

| 维度 | Pass 条件 | 工具 |
|---|---|---|
| **perf** | README 含 "vs <竞品>" 表 + 数字来自 `cargo bench` + 当前 stone 至少在一个 use case 领先；如无竞品标"first-in-Rust" + 文档化空白 | criterion |
| **mem** | dhat `Profiler` 录一次典型 op；README 附 peak alloc | `dhat` feature crate |
| **size** | `cargo package --list` + `cargo bloat --release` 顶部 5；README 附 release-strip 后 .rlib 大小 | `cargo-bloat` |
| **doc** | `cargo doc --no-deps -p X 2>&1 \| grep -c warning == 0`；README 含 quickstart + perf table + license | `#![deny(missing_docs)]` |
| **test** | `cargo llvm-cov -p X --summary-only` line cov ≥ **80%** | `cargo-llvm-cov` |
| **bench** | `criterion` bench 存在；`tests/perf_gate.rs` 有 ≥1 gate；BUDGETS.md 记数字 | criterion + gate |

### Part B — v2 cold backlog 清空 (12 项)

| # | 来源 | 任务 | 预估 |
|---|---|---|---|
| 1 | v0.6 finding | mailrs-sieve rewrite 替换 sieve-rs (解 AGPL) | 大（~2000 LOC RFC 5228 impl） |
| 2 | v0.6 OWASP A04 | plaintext IMAP/POP3 加 deprecation marker + env 关闭开关 | 小 |
| 3 | v0.6 OWASP A10 | `/api/proxy/{image,link}` 加 outbound hostname allowlist | 中 |
| 4 | v0.5 cold | OpenAPI 50 stub 补完整 schema | 大 |
| 5 | v0.4 cold | hot path 加 `#[tracing::instrument]` (IMAP handle_line, POP3 handle_line, MCP per-tool) | 小 |
| 6 | v0.4 cold | 现有 tracing normalize `event=` 字段（不一致的） | 中 |
| 7 | v0.2 cold | render_preview, inline_image, webhook delivery, event_bus, dmarc_report, web/auth/oidc 二次 audit | 中 |
| 8 | v0.1 cold | bench infra: 加 inbound pipeline + PG/Valkey 进 smtp_load bench | 大 |
| 9 | v0.3 cold | 把旧手写 prometheus 文本生成迁移到 metrics-rs facade | 中 |
| 10 | task #112 | 监控 SPF/DKIM/ARC/DMARC shadow divergence + drop mail-auth | 中（被动） |
| 11 | DEPS_AUDIT 持续 | re-audit `mail-builder` 是否值得替换 | 小 |
| 12 | docs/ 整理 | 把 docs/login-golia-jp-integration.md 之类临时 doc 整理掉 | 小 |

## L4 Triggers (Cold → Hot 升级)

| From → To | Trigger 条件 |
|---|---|
| v3.1 → v3.2 | Tier A 12 stones 全部 6 维度过 + 至少 1 个 release（重测 perf 数字 + 更新 README/BUDGETS）|
| v3.2 → v3.3 | Tier B 21 stones 全部 6 维度过 |
| v3.3 → v3.4 | Tier C 7 stones 全部 6 维度过 |
| v3.4 → v3.5 | v2 cold backlog 12 项全 closed（含"refused" / "deferred to v4" 文档化）|
| v3.5 → v3.6 | mail-auth runtime drop（task #112 完成）|
| v3.6 → done | ARCHITECTURE.md cement 表二次终审：无 stone-shaped 剩余 |

## L3a v3.1 Hot 计划 (当前活跃 checkpoint)

Tier A 12 stones，每个走"6 维度审查 + 竞品对比 + 修补"流程。建立可重复
模板 (`scripts/stone-audit.sh`)，后续 Tier B/C 复用。

| # | 步骤 | 检测命令 |
|---|---|---|
| 1 | 写 `scripts/stone-audit.sh`: 自动跑 cargo doc / cov / bench / package size，输出每个 stone 的 6 维度报告 | script 存在 + 在 mailrs-rfc5322 上 dry-run 出报告 |
| 2 | 在第一个 stone (`mailrs-rfc5322`) 上跑全流程：调研竞品 (mail-parser / Go net/mail / C libmail) → bench 对比 → 写 README "vs competitor" 表 → cov ≥80% → release patch | README 含 perf 表 + CHANGELOG 记本次审查 + crate publish 通过 |
| 3-14 | 对剩 11 个 Tier A stones 重复上述流程：mailrs-arc, mailrs-arf, mailrs-dkim, mailrs-ical, mailrs-imap-proto, mailrs-mime, mailrs-mta-sts, mailrs-rfc2047, mailrs-smtp-proto, mailrs-spf, mailrs-tls-rpt | 每个 stone publish 一个 patch；REFACTOR-V3-tier-a.md 表 12/12 ✅ |
| 15 | 整组合并到 ARCHITECTURE.md：每 stone 加 "vs X (Y%)" 标注；更新 PERFORMANCE.md ledger | ARCHITECTURE.md 12 行更新 |
| 16 | `./scripts/release.sh` 发 server patch 验证整组通过 | tag 推到 origin |

## L3b v3 Cold 计划 (本版本剩余 — 不写 step 级)

### v3.2 — Tier B 21 stones
- 同 v3.1 模板，每个补 fuzz target（如果是 parser）+ 竞品 perf 对比
- 候选 fuzz 入口：parser 类（rfc2231 / dnsbl / dmarc / dav）必加；其他按
  attacker-reachable surface 判断
- 大文件 stones (`mailrs-clean` 557 / `mailrs-postmaster` 740) 同步审 v2
  的 file-size hard rule

### v3.3 — Tier C 7 stones
- 重点磨：`mailrs-imap-format` (799 prod LOC) — 实际可能藏 god-fn
- 从头补 BUDGETS / CHANGELOG / bench / perf_gate / fuzz
- `mailrs-delivery-executor` 已是 server perf 主功能，bench 必加

### v3.4 — v2 cold backlog
- 见上表 12 项，按 ROI 排序：A04 deprecation marker (10 min) → A10 SSRF
  allowlist (1h) → tracing instrument (1h) → ... → Sieve rewrite (最大)

### v3.5 — task #112 mail-auth drop
- 看 prod shadow divergence log 累计；判断 cut-over 阈值
- 删 `mail-auth` 依赖；server cargo tree 验证无 mail-auth 残留

### v3.6 — ARCHITECTURE.md 终审
- 整 cement 表再过 lens；记录"经审计仍为 cement，因 X" or "提到 v4 抽
  作 stone"
- v3 closing: REFACTOR-V3.md 写 close-out summary

## 我的执行准则（继承 v2）

- 不在 hot 中重新规划；step 失败 → 停 + 回报
- 每完成一个 stone 立即 commit；不悄悄扩 scope
- 竞品 perf 数字必须有 reproducible 命令；不存在的不写"first-in-Rust"
- 找不到竞品但确认空白时，文档化"在 crates.io 搜过 X / 没结果 / first-mover"

## 进度日志

| Checkpoint | 完成日 | 关键产出 |
|---|---|---|
| v3.1 (Tier A 12 stones) | — | — |
| v3.2 (Tier B 21 stones) | — | — |
| v3.3 (Tier C 7 stones) | — | — |
| v3.4 (v2 cold backlog) | — | — |
| v3.5 (mail-auth drop) | — | — |
| v3.6 (ARCHITECTURE 终审) | — | — |
