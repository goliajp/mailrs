# v0.3 Prometheus /metrics — 实施记录

> Server Refactor v2 checkpoint v0.3。
> 暴露 `/metrics` HTTP endpoint，遵循 Prometheus 文本格式 0.0.4。
> 跑 `curl http://localhost:3200/metrics` 即可拉取。

## 架构

两层 metric 系统并存（过渡期）：

### 第一层：hand-rolled prometheus 文本生成（旧）

`crates/server/src/web/admin/health.rs::prometheus_metrics` 手工拼字符串
输出 13 个核心指标。直接读 WebState 里的 `AtomicU64` + 现场跑 SQL/Redis。
**优点：** 任何指标只要写在 handler 里就上线，零间接。**缺点：** scrape 时
触发数据库查询（PG `COUNT(*)`、Redis `KEYS rbl:status:*`），单次 scrape
~10-100 ms。

### 第二层：`metrics-rs` facade（新）

`crates/server/src/metrics.rs` 安装 `metrics_exporter_prometheus`
的全局 recorder + 保存 `PrometheusHandle`。业务代码用 `metrics::counter!()`
/ `gauge!()` / `histogram!()` 调用记录指标；scrape 时 handler 调
`metrics_handle.render()` 把 facade 收集的全部指标 append 到旧手写
输出后面。**优点：** scrape O(1)，与业务路径解耦；新指标加入只需
`counter!()` 一行；future 可全面替换旧手写。**缺点：** 新增 2 个依赖
（`metrics` 0.24, `metrics-exporter-prometheus` 0.18）。

## 指标清单（v1.7.27 上线时）

### Hand-rolled (旧手写层)

| 指标 | 类型 | 含义 |
|---|---|---|
| `mailrs_uptime_seconds` | gauge | 进程启动至今秒数 |
| `mailrs_connections_total` | counter | 累计连接数 (SMTP) |
| `mailrs_connections_active` | gauge | 当前活跃连接数 |
| `mailrs_messages_total` | counter | 累计交付消息数 |
| `mailrs_active_sessions` | gauge | 当前活跃 web sessions |
| `mailrs_account_cache_size` | gauge | DomainStore 缓存条目 |
| `mailrs_inbound_verdict_total{verdict="accept|reject|defer|junk"}` | counter | 入站 DATA 决策按 verdict 拆 |
| `mailrs_auth_total{outcome="success|failure"}` | counter | Web 登录尝试按结果拆 |
| `mailrs_queue_pending` / `_delivered` / `_failed` / `_bounced` | gauge | 出站队列状态 |
| `mailrs_health_pg_up` / `_valkey_up` | gauge | PG / Valkey 健康 |
| `mailrs_suppression_count` | gauge | 抑制列表条目数 |
| `mailrs_rbl_listed` | gauge | 当前被 RBL 收录的 IP 数 |

### metrics-rs facade (新层，v0.3 新增)

| 指标 | 类型 | 含义 | 入口 |
|---|---|---|---|
| `mailrs_imap_connections_total` | counter | IMAP 接入连接数 | `imap_session/mod.rs::handle_connection` |
| `mailrs_pop3_connections_total` | counter | POP3 接入连接数 | `pop3_session/connection.rs::handle_connection` |
| `mailrs_mcp_sessions_total` | counter | MCP session 创建数 | `mcp/mod.rs::setup_mcp` (per-session closure) |

后续工作（v0.4+）：把第一层全部迁移到 facade，同时新增 SMTP DATA
per-stage histogram（anti-spam / classify / sieve / local-deliver /
remote-enqueue 各一）。

## 字段命名约定

- prefix: `mailrs_`
- 协议: `smtp_` / `imap_` / `pop3_` / `mcp_` / `queue_` / `auth_` /
  `inbound_` / `outbound_`
- 单位后缀: `_total` (counter), `_seconds` / `_bytes` (gauge / histogram)
- labels 用 snake_case；值用 lowercase

## Trigger 满足

| 类别 | 指标 | 状态 |
|---|---|---|
| SMTP | `mailrs_connections_total`, `mailrs_messages_total`, `mailrs_inbound_verdict_total` | ✅ live |
| IMAP | `mailrs_imap_connections_total` | ✅ v0.3 新增 |
| POP3 | `mailrs_pop3_connections_total` | ✅ v0.3 新增 |
| MCP | `mailrs_mcp_sessions_total` | ✅ v0.3 新增 |
| outbound-queue | `mailrs_queue_pending`, `_delivered`, `_failed`, `_bounced` | ✅ live |

5/5 类，**trigger 满足**。

## 验证

部署后:

```bash
curl -s https://mailrs.golia.jp/metrics | grep "^mailrs_" | head -30
```

应该看到旧手写指标 + (在有 IMAP/POP3/MCP 流量后) 新 facade 指标。
