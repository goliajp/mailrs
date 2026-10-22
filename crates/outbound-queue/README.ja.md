# mailrs-outbound-queue

[![Crates.io](https://img.shields.io/crates/v/mailrs-outbound-queue?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-outbound-queue)
[![docs.rs](https://img.shields.io/docsrs/mailrs-outbound-queue?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-outbound-queue)
[![License](https://img.shields.io/crates/l/mailrs-outbound-queue?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-outbound-queue?style=flat-square)](https://crates.io/crates/mailrs-outbound-queue)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust MTA 向け 送信メールキュー基本要素 —— DKIM 署名、DSN 生成、MTA-STS 参照、リトライ/バックオフ、MX/DANE 対応の配送 worker。ストアは利用側が接続します。

[mailrs] から切り出されており、どんな Rust MTA プロジェクトでも本当に厄介な部分を再利用できます:転送メールへの ARC 署名、規格準拠の Delivery Status Notification の生成、MTA-STS ポリシーの参照、ジッタ付き指数バックオフの計算、「5xx は諦めるべきか後で再試行すべきか」という長い裾野の判定。

## 特徴

- **ストレージは利用側が用意** —— キューの状態は [`QueueStore`] トレイト、起床通知は [`Notifier`] の背後にあります。crate はデータベース実装を持たず、データベースドライバにも依存しません。
- **実行できる契約** —— [`store::contract::check_queue_store`] が、worker が前提とする規則（アトミックな取得、再試行とバウンスの状態遷移、クラッシュ復旧、管理操作、抑止リスト）に沿って実装を検査します。テストから一度呼ぶだけです。
- **配送 worker 同梱** —— [`DeliveryWorker`] は任意の `QueueStore` 上で「取得 → 配送」ループを回します。MX 解決、DANE / MTA-STS に従う STARTTLS（[`mailrs-smtp-client`] 経由）、DKIM 署名、転送メールの ARC シール、バックオフ再試行、DSN、ハードバウンス抑止を含みます。
- **リファレンス実装** —— [`InMemoryQueueStore`] と [`InMemoryNotifier`] は契約検査を通る完全な実装で、テストや単一プロセス構成に使えます。
- **純粋なプリミティブ** —— `dkim_sign`、`dsn`、`mta_sts`、`retry` は単体でも使えます。

## クイックスタート

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


ソケットを開かずにストアのライフサイクルだけを追う例は [`examples/in_memory_queue.rs`](examples/in_memory_queue.rs) にあります。

## 独自ストアの接続

1. 自前のバックエンドで [`QueueStore`] を実装します。`claim_for_delivery` はアトミックでなければなりません（同時に二つ取得しても同じ行は返らない）。これで複数の worker が一つのストアを共有できます。SQL なら `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED) RETURNING …` の一文です。
2. すべての worker プロセスに届く仕組み（pub/sub チャネル、データベースの `LISTEN`）で [`Notifier`] を実装するか、[`NoopNotifier`] を渡して `WorkerConfig::poll_interval_secs` のポーリングだけに頼ります。
3. ストアを検査します：

```rust,ignore
#[tokio::test(flavor = "multi_thread")]
async fn my_store_meets_the_contract() {
    mailrs_outbound_queue::store::contract::check_queue_store(|| async {
        MyStore::connect_to_an_empty_test_database().await
    })
    .await;
}
```


## モジュール一覧

| モジュール   | 備考 |
|--------------|------|
| `store`      | `QueueStore`、`Notifier`、`NoopNotifier`、`InMemoryQueueStore`、`InMemoryNotifier`、`StoreError`、`contract`。 |
| `queue`      | `QueuedMessage`、`QueueStatus`、`is_hard_bounce`。 |
| `worker`     | `DeliveryWorker`、`deliver_domain_static`、`try_deliver_via_mx`。 |
| `dkim_sign`  | RFC 6376 DKIM 署名 + RFC 8617 ARC シール。 |
| `dsn`        | RFC 3464 / 6533 DSN 生成。 |
| `mta_sts`    | RFC 8461 MTA-STS ポリシー取得。 |
| `retry`      | バックオフ計算 + バウンス判定。 |

## この crate が「やらない」こと

- SMTP **サーバー** ではない —— 受信側ステートマシンは [`mailrs-smtp-proto`] を参照。
- DKIM **検証** はしない —— ここでは送信側 (署名) のみ。検証は [`mail-auth`]。
- 受信時の SPF / DMARC 強制はしない。それらはこの crate より上流。
- メッセージ保存 / スレッディングはしない。`mailrs-mailbox` / `mailrs-maildir` を参照。

## ライセンス

下記いずれかを選択:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ライセンス ([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

明示的に別段の意思表示をしない限り、本プロジェクトへの貢献は Apache-2.0 ライセンスの定義に従い、追加条件なしで上記の二重ライセンスで配布されるものとする。

[RFC 8617]: https://datatracker.ietf.org/doc/html/rfc8617
[mailrs]: https://github.com/goliajp/mailrs
[`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[`mail-auth`]: https://crates.io/crates/mail-auth
