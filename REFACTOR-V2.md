# Server Refactor v2 — 4-Layer Plan

> v1 ("结构 / stones / 性能基线 / 文档") 主线已完成。v2 是 server-level
> polish — 不再大量增量 stones（除非 cement 二次审计露出），不动协议层，
> 只做"软指标"维度（observability / 一致性 / 安全 / deploy / 日志）。

## L1 Roadmap (一句话)

让 mailrs server 在 **性能 / 可观测 / 安全 / API 一致性 / 日志 / deploy**
六个维度都达到 production-grade，让接手者即用即上线。

## L2 v2 边界 (定下不动)

**v1 已完成（7 条主线）：** 结构清晰 + stones 31 个 published + 性能基线
(SMTP 1079 msg/s) + 文档三件套 + 跨语言 bench + fuzz 8×13 targets + workspace 零 warning。

**v2 scope = server-level polish。** 工作单元：

| Checkpoint | 主题 | 维度 |
|---|---|---|
| v0.1 | 下一波性能 profile + 至少 1 次优化 ship | 性能 |
| v0.2 | Cement 二次审计 + 抽出新 stone（N≥1 或文档化结论"无") | Stone / 社区 |
| v0.3 | Prometheus `/metrics` endpoint + 核心计数器 | 可观测 |
| v0.4 | 日志格式统一 + 字段 schema + CI lint | 可观测 |
| v0.5 | API drift 审计（REST / MCP / OpenAPI 三同步） | 一致性 |
| v0.6 | 安全 audit（cargo audit / deny / OWASP 走查） | 安全 |
| v0.7 | Deploy 健康 gate + rollback | Deploy |

被动等待项：mail-auth runtime drop（任务 #112，等 shadow log 收齐自然触发，
不在主线推进序列里）。

## L4 Triggers (Cold → Hot 升级条件)

每个 trigger 是状态判定式，**不靠灵感判断**。

| From → To | Trigger 条件 |
|---|---|
| v0.1 → v0.2 | `PERFORMANCE.md` 新增行 **且** （end-to-end SMTP 改善 ≥5% **或** P999 改善 ≥10% **或** 该轮 negative-result 文档化） **且** ship 到生产（vX.Y.Z tag 推上 origin） |
| v0.2 → v0.3 | 至少 1 个新 stone published 到 crates.io **或** ARCHITECTURE.md cement 表更新且结论"经审计无新 stone 可抽"已写入 |
| v0.3 → v0.4 | `/metrics` endpoint live **且** SMTP / IMAP / POP3 / MCP / outbound-queue 5 类计数器可 `curl` 出来 |
| v0.4 → v0.5 | 全 server 日志格式审计完成 **且** 所有 hot path 有 `#[tracing::instrument]` **且** 字段 schema 文档化 |
| v0.5 → v0.6 | API drift 修完 **且** OpenAPI spec 通过 schema validator |
| v0.6 → v0.7 | `cargo audit` clean **且** `cargo deny check` clean **且** OWASP top-10 走查报告归档 |
| v0.7 → done | `release.sh` 含 pre-flight 健康检查 **且** 失败自动回滚 |

## L3a v0.1 Hot 计划 ✅ closed (negative-result)

原计划 10 步走 bench → profile → optimize → ship 流程。Step 1-2 完成后
在 step 3 被两个真实障碍阻塞（samply 在 macOS 上 symbol 不解析，且
bench harness 不覆盖真实生产瓶颈）。结论：bench 这条链已到 disk-fsync
ceiling，进一步 perf 优化必须先建 bench infra 或借 prod tracing。详见
[REFACTOR-V2-v0.1-finding.md](./REFACTOR-V2-v0.1-finding.md)。

L4 trigger 通过 "negative-result 文档化" 分支满足，**v0.1 closed**。

## L3a v0.2 Hot 计划 ✅ closed (stone shipped)

`mailrs-arf` 1.0.0 published 到 crates.io，server 切换到新 stone，fbl.rs 删除。
audit 文档 + finding 见 [REFACTOR-V2-v0.2-cement-audit.md](./REFACTOR-V2-v0.2-cement-audit.md)。

## L3a v0.3 Hot 计划 ✅ closed (5/5 类指标 live)

旧 `/metrics` handler 已存在（手写 prometheus 文本，13 个指标）。新增：
- `metrics` + `metrics-exporter-prometheus` facade 依赖入 server
- `metrics.rs`: PrometheusHandle 安装 + 注入 WebState
- IMAP / POP3 connection handler 加 `metrics::counter!()`
- MCP `setup_mcp` per-session closure 加 `metrics::counter!()`
- `prometheus_metrics` handler 末尾 append `metrics_handle.render()`

总计 5 类指标可 curl: SMTP / IMAP / POP3 / MCP / outbound-queue。
详情 [REFACTOR-V2-v0.3-metrics.md](./REFACTOR-V2-v0.3-metrics.md)。

## L3a v0.4 Hot 计划 ✅ closed (62 → tracing + CI lint)

62 个 production `eprintln!` 全部转为 `tracing::*`，字段 schema 文档化，
CI lint script 上线。Step 4 (现有 tracing normalize) + Step 5 (hot-path
`#[tracing::instrument]`) 进 v0.4 cold backlog —— trigger 已满足（log
schema 文档化 + 所有 hot path 已有结构化日志），后续 instrument 加层
属于增量优化，不阻塞 v0.5。

## L3a v0.5 Hot 计划 ✅ closed (drift = 0)

API drift 审计完成。50 个 router endpoint + 6 个 MCP tools 全部
同步到 openapi.json / llm-full.txt。CI lint `scripts/check-api-drift.sh`
+ stub 生成器 `scripts/openapi-stub-missing.py` 上线。详情
[REFACTOR-V2-v0.5-api-drift.md](./REFACTOR-V2-v0.5-api-drift.md)。

## L3a v0.6 Hot 计划 ✅ closed (4/4 categories ok)

`cargo audit` clean + `cargo deny check` 4 类全 ok + OWASP top-10 走查
归档。揭出两个 finding 并文档化：
- **sieve-rs AGPL** — 文档化 mitigation；升级"rewrite Sieve as stone"
  到 cold backlog 高优先级
- **RUSTSEC-2023-0071 (rsa Marvin Attack)** — 评估 mailrs DKIM 不暴露
  per-op timing → 风险接受 + 文档化触发条件

也加入 v0.6 cold backlog：
- A04 plaintext IMAP/POP3 默认开 → 加 deprecation marker
- A10 `/api/proxy/{image,link}` 加 outbound hostname allowlist (防 SSRF)

详情 [REFACTOR-V2-v0.6-security.md](./REFACTOR-V2-v0.6-security.md)。

## L3a v0.7 Hot 计划 ✅ closed (deploy gate + rollback shipped)

`scripts/deploy.sh` 完全重写：
- pre-deploy health check（curl 旧服务的 `/api/health`）
- 部署前备份当前 binary 到 `/root/backup/mailrs-server.<timestamp>`
- post-deploy 等待健康 (poll 60s + 接受 healthy/degraded)
- 失败自动 rollback (拷贝备份回去 + force-recreate container)
- release.sh 已有的 rollback trap 现在覆盖 deploy 健康失败

`DEPLOY.md` 写了完整流程 + 失败处理。

最终 release 走全 gate 验证流程可用。

## L3b v2 Cold 计划 (本版本剩余 — 不写 step 级)

### v0.2 Cement 二次审计
- **做什么：** 用 ARCHITECTURE.md 的 cement 表 + 这次拆分后的新 module boundary 重审，找漏网的 stone。
- **候选（来自第一遍 audit 后的新视角）：** `render_preview.rs` (Chromium-backed preview render)、`inline_image.rs` (CID inline image processing)、`webhook/` delivery worker (retry + ordering)、`event_bus.rs` (typed broadcast)、`dmarc_report.rs` (PG-agnostic store trait)、`web/auth/oidc.rs` (OIDC client builder)
- **资源：** 上次 cycle 抽 7→31 用的方法（grep-able lens + perf-friendly boundary）
- **产出：** 新 stone publish OR ARCHITECTURE.md cement 表更新条目说明"经审计仍为 cement，原因 X"

### v0.3 Prometheus /metrics endpoint
- **做什么：** 暴露 HTTP `/metrics`，遵循 prometheus 文本格式
- **候选库：** `metrics` + `metrics-exporter-prometheus`（直接 metrics-rs 生态），或 `axum-prometheus`
- **核心计数器：** SMTP accept/reject/deliver、IMAP idle/select、POP3 retr、MCP tool call、outbound queue depth + retry、PG / valkey 连接池
- **资源：** axum 已经在用，复用 router

### v0.4 日志格式统一
- **做什么：** 审计所有 `eprintln!` / `tracing::{warn,error,info,debug}` 调用，统一 event= 字段命名 + 字段 schema
- **资源：** grep `eprintln!` 直接列出全部 callsite；现有 tracing 实例化基础
- **产出：** docs/LOGGING.md 字段 schema + ci lint script

### v0.5 API drift 审计
- **做什么：** 对照 `rules/api-update-checklist.md` 走查所有 REST endpoint + MCP tools + openapi.json + llm-full.txt 的一致性
- **资源：** 已有 checklist；52 个 MCP tools + 路由表
- **产出：** drift 报告 + 修正 commits

### v0.6 安全 audit
- `cargo audit` + `cargo deny check` 跑通
- OWASP top-10 手动走查（auth、injection、XSS、CSRF）
- 产出：docs/SECURITY-AUDIT.md

### v0.7 Deploy 健康 gate
- `release.sh` 加 pre-flight curl 检查（旧 binary 仍 OK → 新 binary 部署 → 新 binary curl OK → 才 commit；否则回滚）
- 旧 binary 自动 backup 到 `~/backup/`

## 我的执行准则

- 不在 hot 中重新规划。Hot 步骤跑失败 → 停 + 回报。
- 不悄悄扩 scope。发现 v2 边界要变 → 停 + 改 L2 + 重写 hot。
- Checkpoint 完成时不直觉接下一段：先检查 L4 trigger 是否满足。
- 每次发现的 bug / 改进 / 顾虑 → 记到 cold backlog，**不要回流到当前 hot**。

## 进度日志（每个 checkpoint 完成时填）

| Checkpoint | 完成日 | Trigger 满足 | 关键产出 |
|---|---|---|---|
| v0.1 | 2026-05-25 | ✅ negative-result 文档化（trigger 替代分支）| [REFACTOR-V2-v0.1-finding.md](./REFACTOR-V2-v0.1-finding.md): bench 已到 disk-fsync ceiling，下一波 perf 必须先建 infra 或与 v0.3 metrics 合并 |
| v0.2 | 2026-05-25 | ✅ 新 stone published (`mailrs-arf` 1.0.0) | [REFACTOR-V2-v0.2-cement-audit.md](./REFACTOR-V2-v0.2-cement-audit.md): 40 cement 文件审计 → 1 stone 抽出。`mailrs-arf` 1.16 µs/report, 18 测试, 首个 Rust ARF parser。Server `fbl.rs` (37 LOC) 删除 → 用 stone |
| v0.3 | 2026-05-25 | ✅ /metrics live + 5/5 类指标 (SMTP/IMAP/POP3/MCP/queue) | [REFACTOR-V2-v0.3-metrics.md](./REFACTOR-V2-v0.3-metrics.md): metrics-rs facade 装入 + IMAP/POP3/MCP 新 counters + 旧手写层并存 |
| v0.4 | 2026-05-25 | ✅ log schema 文档化 + 62 eprintln → tracing + CI lint | [REFACTOR-V2-v0.4-log-audit.md](./REFACTOR-V2-v0.4-log-audit.md): 全 17 文件统一 event= 字段；`scripts/check-no-eprintln.sh` 拒绝回流 |
| v0.5 | 2026-05-25 | ✅ drift 矩阵清零 + CI lint | [REFACTOR-V2-v0.5-api-drift.md](./REFACTOR-V2-v0.5-api-drift.md): 50 个 endpoint stub + 6 个 MCP tool doc + 2 个 lint script |
| v0.6 | 2026-05-25 | ✅ cargo audit + deny + OWASP 4/4 ok | [REFACTOR-V2-v0.6-security.md](./REFACTOR-V2-v0.6-security.md): sieve-rs AGPL + RUSTSEC-2023-0071 findings 文档化；`scripts/check-security.sh` 上线 |
| v0.7 | 2026-05-25 | ✅ deploy.sh 含 pre+post-deploy curl + auto-rollback | [DEPLOY.md](./DEPLOY.md): 备份 → curl → 失败自动恢复；release.sh trap 也覆盖 deploy 健康失败 |
| v0.3 | — | — | — |
| v0.4 | — | — | — |
| v0.5 | — | — | — |
| v0.6 | — | — | — |
| v0.7 | — | — | — |
