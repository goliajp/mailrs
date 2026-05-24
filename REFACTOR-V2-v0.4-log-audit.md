# v0.4 日志格式统一 — Audit + Schema

> Server Refactor v2 checkpoint v0.4。
> 目标：把散落的 `eprintln!` 全部转成 `tracing::{error,warn,info,debug}!`，
> 定义 `event=` 字段 schema，加 CI lint 防回流。

## Baseline (2026-05-25)

| 项 | 数 |
|---|---|
| `eprintln!` callsites | **64** |
| `println!` callsites (生产代码) | 0 |
| `tracing::{error,warn,info,debug}!` callsites | 159 |

按文件分布（top 10）：

| 文件 | eprintln 数 |
|---|---|
| `ai_analyzer.rs` | 11 |
| `web/mail/send/text.rs` | 10 |
| `content_worker.rs` | 5 |
| `bootstrap/runtime_tasks.rs` | 5 |
| `search_index.rs` | 4 |
| `main.rs` | 4 |
| `web/templates.rs` | 3 |
| `web/mail/preview.rs` | 3 |
| `smtp_session/events/data/mod.rs` | 3 |
| `render_preview.rs` | 3 |

## 字段 Schema (现在文档化，未来代码强制)

所有 tracing 调用使用以下字段命名：

| 字段 | 类型 | 含义 | 例 |
|---|---|---|---|
| `event` | static str | 事件类型，snake_case | `event = "smtp_data"` |
| `conn_id` | u64 | 连接 ID（uniquely tracked per accept） | `conn_id = 42` |
| `user` | %String | 用户邮箱地址 | `user = %addr` |
| `error` | %display | 错误对象 display 形式 | `error = %e` |
| `error_dbg` | ?Debug | 错误对象 debug 形式（少用） | `error_dbg = ?e` |
| `phase` | static str | 子阶段名 | `phase = "inbound_pipeline"` |
| `duration_us` | u64 | 子阶段耗时 (µs) | `duration_us = 123` |
| `rcpt` | %String | 收件人 | `rcpt = %r` |
| `from` | %String | 发件人 | `from = %sender` |
| `domain` | %String | 域名 | `domain = %d` |
| `path` | %String | 文件路径 | `path = %p` |
| `reason` | %String | 业务原因 | `reason = %why` |
| `count` | u64 | 计数 | `count = n` |

约定：
- 字段在前，message string 在后
- 不要在 message string 里重复字段值（如 `"smtp data for {user}"` 加 `user = %u` 是冗余）
- message 用一句话英文，不带换行
- 同一个 callsite 用同一 `event=` 值，多次出现是同一个事件

## Level 选择启发式

- **error!** — 数据丢失风险、用户操作失败、协议致命错误（DB 写失败、deliver 失败、auth 配置错误）
- **warn!** — 异常但可恢复、配置缺失走 fallback、协议非致命错误（连接超时、重试已发起、quota 接近）
- **info!** — 业务里程碑、生命周期事件（启动、停止、subsystem 启用、用户登录成功）
- **debug!** — 细节、调试用（每条消息的 stage 计时、缓存命中/未命中）

## 转换批次（全部在一个 commit 里完成，2026-05-25）

| 批次 | 文件 | 处理数 |
|---|---|---|
| critical path | imap_session, smtp_session/events/data, bootstrap/{runtime_tasks, outbound, web_state} | 13 |
| background workers | ai_analyzer (11), content_worker (5), rbl_monitor (2), render_preview (3), search_index (4) | 25 |
| web handlers | web/mail/send/text (10), preview (3), drafts (2), messages (2), templates (3) | 20 |
| misc | main.rs (4) | 4 |

**总计：62 个 eprintln! 转 tracing**，剩 2 个测试块内的 eprintln（test/bench
nocapture print）保留。CI lint script `scripts/check-no-eprintln.sh`
排除测试块，pre-flight 强制 0 命中。

## CI lint

完成后加 `scripts/check-no-eprintln.sh`：

```bash
#!/usr/bin/env bash
# Forbid eprintln! in production server code.
# Tests, benches, and main.rs early-boot are allowed.
set -euo pipefail
hits=$(grep -rn 'eprintln!' crates/server/src \
  --include='*.rs' \
  --exclude-dir=tests \
  --exclude-dir=benches \
  | grep -vE '/tests/|/benches/' || true)
if [ -n "$hits" ]; then
  echo "ERROR: forbidden eprintln! in production code:"
  echo "$hits"
  exit 1
fi
```

提到 README 的 pre-flight checklist。
