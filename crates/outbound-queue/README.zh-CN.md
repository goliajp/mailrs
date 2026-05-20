# mailrs-outbound-queue

[![Crates.io](https://img.shields.io/crates/v/mailrs-outbound-queue?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-outbound-queue)
[![docs.rs](https://img.shields.io/docsrs/mailrs-outbound-queue?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-outbound-queue)
[![License](https://img.shields.io/crates/l/mailrs-outbound-queue?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-outbound-queue?style=flat-square)](https://crates.io/crates/mailrs-outbound-queue)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust MTA 的出站邮件队列原语 —— DKIM 签名、DSN 生成、MTA-STS 查询、重试退避、MX/DANE 投递, 加上可插拔的 store trait + Postgres 参考实现。

从 [mailrs] 抽出, 让任意 Rust MTA 项目都能复用真正难做的部分:用 ARC 给转发邮件签名、生成符合标准的退信回执、查 MTA-STS 策略、计算带抖动的指数退避, 以及那条 "5xx 到底是放弃还是稍后重试" 的判断长尾。

## 亮点

- **Trait 化的 store** —— [`QueueStore`] + [`Notifier`] 解耦队列状态和具体后端。自带 [`InMemoryQueueStore`] 用于测试 / 试点。
- **Postgres 参考实现** —— [`PgQueueStore`] + [`RedisNotifier`] (默认 `pg` feature) 直接对接 mailrs 用的 schema, 一个构造器就有生产级队列。
- **纯逻辑原语, 不需要数据库** —— `dkim_sign` / `dsn` / `mta_sts` / `retry` 全是纯逻辑。关掉 `pg` feature 它们照样能编能用。
- **转发邮件 ARC 封印** —— `dkim_sign::arc_seal_message` 在 DKIM 旁边加上 ARC 链 ([RFC 8617]), 下游过滤器可以信任转发链路。
- **自带投递 worker** —— [`DeliveryWorker`] 是 poll-and-deliver 循环, 走 MX 记录, 强制 DANE TLSA ([`mailrs-smtp-client`]). v1.0.0 是 PG-only; trait 之上的泛型 worker 是 v2 规划。
- **生产环境验证** —— 从一个 Rust 邮件服务器拆出。

## 快速开始 (PG 后端)

```rust,no_run
use mailrs_outbound_queue::{DeliveryWorker, PgQueueStore, QueueStore, WorkerConfig};
use std::sync::Arc;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;

// 通过 trait 入队 …
let store = Arc::new(PgQueueStore::new(pool.clone())) as Arc<dyn QueueStore>;
let id = store
    .enqueue(
        "sender@example.org", "bob@example.com", "example.com",
        b"Subject: hi\r\n\r\nhello\r\n", None,
        chrono::Utc::now().timestamp(), false,
    )
    .await?;
println!("queued message #{id}");

// … 或跑自带 worker (Postgres + Redis) 把队列消费掉
let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()?.build()?;
let worker = DeliveryWorker::new(WorkerConfig::default(), pool, resolver, "smtp.example.org".into());
let (_tx, rx) = tokio::sync::watch::channel(false);
worker.run(rx).await;
# Ok(()) }
```

无 DB 完整 trait 操作流程见 [`examples/in_memory_queue.rs`](examples/in_memory_queue.rs)。

## Feature flags

| Feature | 默认 | 启用什么 |
|---------|------|---------|
| `pg`    | on   | `PgQueueStore`, `RedisNotifier`, 自带的 `DeliveryWorker`。引入 `sqlx` (Postgres) + `redis`。 |

要纯 trait 构建:

```toml
mailrs-outbound-queue = { version = "1", default-features = false }
```

这种模式下你得到 `QueueStore` + `Notifier` + `InMemoryQueueStore` + `InMemoryNotifier` + 纯逻辑原语 (`dkim_sign` / `dsn` / `mta_sts` / `retry`)。worker 自己写。

## 模块速览

| 模块         | 总是有 | 备注 |
|--------------|--------|------|
| `store`      | yes    | `QueueStore`, `Notifier`, `InMemoryQueueStore`, `InMemoryNotifier`, `StoreError`。 |
| `queue`      | yes    | `QueuedMessage`, `QueueStatus`, `is_hard_bounce`。PG free fn (在 `pg` 之后)。 |
| `dkim_sign`  | yes    | RFC 6376 DKIM 签名 + RFC 8617 ARC 封印。 |
| `dsn`        | yes    | RFC 3464 / 6533 退信回执生成。 |
| `mta_sts`    | yes    | RFC 8461 MTA-STS 策略查询。 |
| `retry`      | yes    | 退避调度 + bounce 判定。 |
| `pg_store`   | `pg`   | `PgQueueStore` + `RedisNotifier`。 |
| `worker`     | `pg`   | `DeliveryWorker` poll-and-deliver 循环。 |

## API 两条路径

同一套队列语义, 提供两条并行的公开接口:

- **Trait API** (`QueueStore` / `Notifier`) —— 可移植接口。想接非 PG 后端, 或者想自己控制 delivery loop, 走这条。
- **PG free 函数** (`queue::` 模块下) —— 常见场景的便利通道:已经有 `sqlx::PgPool` 直接 `queue::enqueue(pool, ...)`。自带 `DeliveryWorker` 走这条, mailrs 自己也走这条。

v1.x 两套都稳定向后兼容。v2 计划是合到 trait 表面 + 泛型 worker, 但不指望破坏任何 v1 用户代码。

## 这个 crate 不做什么

- 不做 SMTP **服务端** —— 入站状态机看 [`mailrs-smtp-proto`]。
- 不做 DKIM **验证** —— 这里只签 outbound。验证看 [`mail-auth`]。
- 不做入站 SPF / DMARC 强制。
- 不做消息存储 / 线程化 —— 见 `mailrs-mailbox` / `mailrs-maildir`。

## 许可证

双许可证, 二选一:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT 许可证 ([LICENSE-MIT](LICENSE-MIT) 或 <https://opensource.org/licenses/MIT>)

除非你明确声明, 任何提交到本项目的贡献, 按 Apache-2.0 的定义, 都将以上述双许可证发布, 没有任何附加条款。

[RFC 8617]: https://datatracker.ietf.org/doc/html/rfc8617
[mailrs]: https://github.com/goliajp/mailrs
[`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[`mail-auth`]: https://crates.io/crates/mail-auth
