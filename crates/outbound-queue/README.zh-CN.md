# mailrs-outbound-queue

[![Crates.io](https://img.shields.io/crates/v/mailrs-outbound-queue?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-outbound-queue)
[![docs.rs](https://img.shields.io/docsrs/mailrs-outbound-queue?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-outbound-queue)
[![License](https://img.shields.io/crates/l/mailrs-outbound-queue?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-outbound-queue?style=flat-square)](https://crates.io/crates/mailrs-outbound-queue)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust MTA 的出站邮件队列原语 —— DKIM 签名、DSN 生成、MTA-STS 查询、重试退避、MX/DANE 投递 worker，存储由使用方接入。

从 [mailrs] 抽出, 让任意 Rust MTA 项目都能复用真正难做的部分:用 ARC 给转发邮件签名、生成符合标准的退信回执、查 MTA-STS 策略、计算带抖动的指数退避, 以及那条 "5xx 到底是放弃还是稍后重试" 的判断长尾。

## 亮点

- **存储由使用方提供** —— 队列状态放在 [`QueueStore`] trait 后面，唤醒放在 [`Notifier`] 后面。crate 不带数据库实现，也不引入任何数据库驱动。
- **契约可以直接跑** —— [`store::contract::check_queue_store`] 按 worker 依赖的每条规则检查你的实现：原子领取、重试和退信的状态转换、崩溃恢复、管理操作、抑制名单。在一个测试里调用即可。
- **自带投递 worker** —— [`DeliveryWorker`] 对任意 `QueueStore` 跑「领取 → 投递」循环：MX 解析、按 DANE / MTA-STS 处理 STARTTLS（经 [`mailrs-smtp-client`]）、DKIM 签名、转发邮件的 ARC 封印、退避重试、退信回执和硬退信抑制。
- **参考实现** —— [`InMemoryQueueStore`] 和 [`InMemoryNotifier`] 是完整、通过契约检查的实现，适合测试和单进程部署。
- **纯逻辑原语** —— `dkim_sign`、`dsn`、`mta_sts`、`retry` 可以单独使用。

## 快速开始

```rust,no_run
use std::sync::Arc;
use mailrs_outbound_queue::{
    DeliveryWorker, InMemoryNotifier, InMemoryQueueStore, Notifier, QueueStore, WorkerConfig,
};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let store = Arc::new(InMemoryQueueStore::new());
let notifier = Arc::new(InMemoryNotifier::new());

let id = store
    .enqueue(
        "sender@example.org", "bob@example.com", "example.com",
        b"Subject: hi\r\n\r\nhello\r\n", None,
        chrono::Utc::now().timestamp(), false,
    )
    .await?;
notifier.notify().await;
println!("queued message #{id}");

let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()?.build()?;
let worker = DeliveryWorker::new(
    WorkerConfig::default(), store, notifier, resolver, "smtp.example.org".into(),
);
let (_stop, stopped) = tokio::sync::watch::channel(false);
worker.run(stopped).await;
# Ok(()) }
```


不开网络、只走存储生命周期的例子见 [`examples/in_memory_queue.rs`](examples/in_memory_queue.rs)。

## 接入自己的存储

1. 在你的后端上实现 [`QueueStore`]。`claim_for_delivery` 必须是原子的：两次并发领取不会拿到同一行，这样多个 worker 可以共用一个存储。用 SQL 写就是一条 `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED) RETURNING …`。
2. 用能通知到每个 worker 进程的东西实现 [`Notifier`]（发布订阅频道、数据库的 `LISTEN`），或者传 [`NoopNotifier`]，只靠 `WorkerConfig::poll_interval_secs` 轮询。
3. 检查存储：

```rust,ignore
#[tokio::test(flavor = "multi_thread")]
async fn my_store_meets_the_contract() {
    mailrs_outbound_queue::store::contract::check_queue_store(|| async {
        MyStore::connect_to_an_empty_test_database().await
    })
    .await;
}
```


## 模块速览

| 模块         | 备注 |
|--------------|------|
| `store`      | `QueueStore`、`Notifier`、`NoopNotifier`、`InMemoryQueueStore`、`InMemoryNotifier`、`StoreError`、`contract`。 |
| `queue`      | `QueuedMessage`、`QueueStatus`、`is_hard_bounce`。 |
| `worker`     | `DeliveryWorker`、`deliver_domain_static`、`try_deliver_via_mx`。 |
| `dkim_sign`  | RFC 6376 DKIM 签名 + RFC 8617 ARC 封印。 |
| `dsn`        | RFC 3464 / 6533 退信回执生成。 |
| `mta_sts`    | RFC 8461 MTA-STS 策略查询。 |
| `retry`      | 退避调度 + 退信判定。 |

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
