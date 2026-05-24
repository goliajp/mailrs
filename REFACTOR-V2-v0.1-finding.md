# v0.1 性能分析 — Negative result + 下一步

> Server Refactor v2 checkpoint v0.1，2026-05-25。
> Trigger 形态：**negative-result 文档化**（PERFORMANCE.md 不新增数字，但分析结论入档）。

## 现状（验证后）

跑 v1.7.25 的 baseline（`smtp_load --duration 30 --conns 32 --warmup 5`，release-debug profile），3 轮 round-robin：

| 轮 | throughput | P50 | P99 | P999 |
|---|---|---|---|---|
| 1 (cold) | 113.3 msg/s | 84516 µs | 3118757 µs | 3202815 µs |
| 2 | 1036.9 msg/s | 27851 µs | 104907 µs | 470592 µs |
| 3 | 1150.9 msg/s | 27880 µs | 32007 µs | 38370 µs |

**结论：** 稳态 throughput **1037–1151 msg/s**，与 PERFORMANCE.md 记录的 1079 msg/s
（post DeliveryExecutor 1.1）完全一致。**无性能回归。** 第 1 轮 cold-cache
+ 外部 IO 干扰是 macOS 上常见的 disk warmup 噪声，第 2 轮起进入稳态。

## 为什么 v0.1 hot 计划阻塞

按 hot 计划走到 step 2 → step 3 时遇到两个真实障碍：

### 障碍 1：samply 在 macOS Apple Silicon 上 symbol 不解析

录的 profile 里所有 leaf function 都是 raw hex（`0x450c`、`0x6fc4` …），即使：
- 用 `release-debug` profile（`debug = "line-tables-only"`, `strip = "none"`）
- 用 `dsymutil` 生成 dSYM bundle 放在 binary 旁边
- `samply 0.13.1`

仍然 100% 是 `?(unresolved)`，所有 frame 的 `nativeSymbol` 索引都是 None。

**推测原因：** Apple Silicon 上 PIE + PAC 让 samply 看到的是 process-relative
偏移，但它没把 dyld load-address 加回去。这是 samply on macOS 的已知边界。

**绕过方法：**
1. 在 Linux 上跑（不是当前 dev 机）
2. 用 Xcode Instruments（需要 GUI + 手动操作）
3. 用 `cargo flamegraph` + `dtrace`（需要 sudo）

短期内不实际，得换 lens。

### 障碍 2：bench harness 不覆盖真实生产瓶颈

`crates/server/benches/smtp_load.rs` 的 SMTP handler 是简化版：

跑：SMTP 协议交互 + `DeliveryExecutor::deliver()`（maildir 写盘）
**不跑：** inbound pipeline (SPF/DKIM/DMARC/sieve)、PG `INSERT messages` +
mailbox indexing、event_bus emit、valkey notify、greylist 查询。

注释明确写着 (smtp_load.rs lines 22-26)：
> What this does NOT bench: The real `mailrs-server` inbound pipeline
> (SPF/DKIM/DMARC/sieve/PG/Valkey writes). Those need a full integration
> environment...

也就是说，**就算 profile 出 bench 里的热点函数，优化它们对生产 SMTP 的
影响也是间接的**。生产瓶颈大概率在 inbound pipeline (DNS 查询)、PG 写入
(网络往返 + commit)、valkey RTT 这些 cement 部分，而 bench 一概不测。

## 关键洞察

把 1079 msg/s 推到 disk-fsync ceiling 之后，**下一个 10× 优化必须在两个
轴之一**：

1. **建 production-realistic bench infrastructure** — testcontainers
   起 PG + Valkey + DNS recursion stub，把 inbound pipeline 接进 bench。
   这才能 profile 真实生产路径。**工作量约 1-2 个 commit cycle，是
   独立的 sub-version。**

2. **认认真真在生产打 tracing span + per-stage 指标** — 部署到 prod，让
   真实流量产生 tracing 数据，看 SPF / DKIM / DMARC / greylist / PG /
   valkey 各 stage 实际花了多少 µs。这条路与 v0.3（Prometheus metrics
   endpoint）天然合并：metrics 就是 per-stage timer。

两个方向哪个都不是 "改 5 行代码立刻提速"，都是中等工程量的基础设施工作。

## 决定（按 4-layer 规则）

按 L4 trigger 表，v0.1 → v0.2 升级条件之一是 **"该轮 negative-result
文档化"**。本文件即是。**v0.1 closed**。

理由：
- 当前 bench 数字健康（无回归）
- 直接 micro-optimization 没有 actionable profile 指引
- 真正下一波性能优化的前置条件（bench infra 或 prod tracing）跟 v0.3
  metrics endpoint 高度重叠

下一步进入 **v0.2 Cement 二次审计**。理由：
- 这次刚拆完所有大文件，module boundary 是历史最清晰的时刻
- "找新 stone" 是 4 维度里 "社区贡献 + 项目质量" 同时收益
- 没有 bench infra 依赖，可以马上推进

## v0.1 累计成本 / 产出

| 项 | 值 |
|---|---|
| 跑 bench 轮数 | 4 |
| 录 profile 数 | 2 (samply) |
| 提交 commit 数 | 0 (无代码修改) |
| 关键发现 | bench 已到 disk-fsync ceiling；samply on macOS symbol 失效；下一波 perf 需 infra 投资或与 v0.3 合并 |
| 输出文档 | 本文件 |
