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

## L3a v0.1 Hot 计划 (当前活跃 checkpoint — 线性、无分叉)

每步带检测命令；失败 → 停，不绕过。

| # | 步骤 | 检测命令 |
|---|---|---|
| 1 | 跑 smtp_load bench 拿 v1.7.25 的最新 baseline | `cargo build --profile release-debug -p mailrs-server --bench smtp_load && $CARGO_TARGET_DIR/release-debug/deps/smtp_load-* --duration 30 --conns 32 --warmup 5` → 记录 mean / P50 / P99 / P999 |
| 2 | 用 `samply` 录 SMTP 接收流 30s，生成 profile | `samply record --duration 30 ...` → `profile.json` 文件存在 |
| 3 | 分析 profile，识别 top 3 占比 hot path（不只看 fsync） | 写到 `docs/v0.1-bottlenecks.md` |
| 4 | 选最高 ROI 优化目标 + 写 criterion micro-bench 测 pre 数字 | `cargo bench` 输出数字 |
| 5 | 实施优化（一次 commit） | `cargo build -p mailrs-server` 零 warn |
| 6 | `cargo test --workspace` 全绿 | 0 failed |
| 7 | 重跑 micro-bench 拿 post 数字 | criterion 输出 |
| 8 | 重跑 smtp_load 拿 end-to-end post 数字 | 与 step 1 同命令 |
| 9 | 更新 `PERFORMANCE.md`，把 v0.1 新行追加 | grep 文件中找到新行 |
| 10 | `./scripts/release.sh` patch 发版 | tag 推到 origin |

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
| v0.1 | — | — | — |
| v0.2 | — | — | — |
| v0.3 | — | — | — |
| v0.4 | — | — | — |
| v0.5 | — | — | — |
| v0.6 | — | — | — |
| v0.7 | — | — | — |
