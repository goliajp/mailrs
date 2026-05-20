# mailrs-outbound-queue

[![Crates.io](https://img.shields.io/crates/v/mailrs-outbound-queue?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-outbound-queue)
[![docs.rs](https://img.shields.io/docsrs/mailrs-outbound-queue?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-outbound-queue)
[![License](https://img.shields.io/crates/l/mailrs-outbound-queue?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-outbound-queue?style=flat-square)](https://crates.io/crates/mailrs-outbound-queue)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust MTA 向け 送信メールキュー基本要素 —— DKIM 署名、DSN 生成、MTA-STS 参照、リトライ/バックオフ、MX/DANE 対応の配送、加えてプラガブルなストア trait と Postgres リファレンス実装。

[mailrs] から切り出されており、どんな Rust MTA プロジェクトでも本当に厄介な部分を再利用できます:転送メールへの ARC 署名、規格準拠の Delivery Status Notification の生成、MTA-STS ポリシーの参照、ジッタ付き指数バックオフの計算、「5xx は諦めるべきか後で再試行すべきか」という長い裾野の判定。

## 特徴

- **Trait プラガブルなストア** —— [`QueueStore`] + [`Notifier`] が配送状態を特定のバックエンドから切り離す。テスト/パイロット向けに [`InMemoryQueueStore`] が同梱。
- **Postgres リファレンス** —— [`PgQueueStore`] + [`RedisNotifier`] (デフォルト `pg` feature) が mailrs と同じスキーマを直接ターゲット。コンストラクタ 1 つで本番品質のキューが手に入る。
- **純粋な基本要素、DB 不要** —— `dkim_sign` / `dsn` / `mta_sts` / `retry` はすべて純ロジック。`pg` feature を切ってもコンパイル・動作する。
- **転送メール向け ARC 封印** —— `dkim_sign::arc_seal_message` が DKIM と並んで ARC チェーンを付け ([RFC 8617])、下流フィルタが転送ホップを信頼できる。
- **配送ワーカー同梱** —— [`DeliveryWorker`] が poll-and-deliver ループを実行し、MX レコードを辿って DANE TLSA を強制 ([`mailrs-smtp-client`])。v1.0.0 は PG 専用、trait 上のジェネリックワーカーは v2 で計画中。
- **実運用で検証済み** —— Rust メールサーバーから切り出し。

## クイックスタート (PG バックエンド)

```rust,no_run
use mailrs_outbound_queue::{DeliveryWorker, PgQueueStore, QueueStore, WorkerConfig};
use std::sync::Arc;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;

// trait 経由でキューに入れる …
let store = Arc::new(PgQueueStore::new(pool.clone())) as Arc<dyn QueueStore>;
let id = store
    .enqueue(
        "sender@example.org", "bob@example.com", "example.com",
        b"Subject: hi\r\n\r\nhello\r\n", None,
        chrono::Utc::now().timestamp(), false,
    )
    .await?;
println!("queued message #{id}");

// … または同梱ワーカー (Postgres + Redis) を起動してキューを消費
let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()?.build()?;
let worker = DeliveryWorker::new(WorkerConfig::default(), pool, resolver, "smtp.example.org".into());
let (_tx, rx) = tokio::sync::watch::channel(false);
worker.run(rx).await;
# Ok(()) }
```

DB なしで trait API をエンドツーエンドで動かす例は [`examples/in_memory_queue.rs`](examples/in_memory_queue.rs) を参照。

## Feature flag

| Feature | デフォルト | 有効になるもの |
|---------|------------|---------------|
| `pg`    | on         | `PgQueueStore`, `RedisNotifier`, 同梱の `DeliveryWorker`。`sqlx` (Postgres) と `redis` を引き入れる。 |

trait のみのビルドにする場合:

```toml
mailrs-outbound-queue = { version = "1", default-features = false }
```

このモードでは `QueueStore` + `Notifier` + `InMemoryQueueStore` + `InMemoryNotifier` + 純粋な基本要素 (`dkim_sign` / `dsn` / `mta_sts` / `retry`) が手に入る。ワーカーは自分で書く。

## モジュール一覧

| モジュール   | 常に  | 備考 |
|--------------|-------|------|
| `store`      | yes   | `QueueStore`, `Notifier`, `InMemoryQueueStore`, `InMemoryNotifier`, `StoreError`。 |
| `queue`      | yes   | `QueuedMessage`, `QueueStatus`, `is_hard_bounce`。PG 自由関数 (`pg` ゲート)。 |
| `dkim_sign`  | yes   | RFC 6376 DKIM 署名 + RFC 8617 ARC 封印。 |
| `dsn`        | yes   | RFC 3464 / 6533 Delivery Status Notification 生成。 |
| `mta_sts`    | yes   | RFC 8461 MTA-STS ポリシー参照。 |
| `retry`      | yes   | バックオフスケジュール + bounce 判定。 |
| `pg_store`   | `pg`  | `PgQueueStore` + `RedisNotifier`。 |
| `worker`     | `pg`  | `DeliveryWorker` poll-and-deliver ループ。 |

## API は 2 つの経路

同じキューセマンティクスに対して、並行する 2 つの公開サーフェスがある:

- **Trait API** (`QueueStore` / `Notifier`) —— ポータブルなインターフェース。非 PG バックエンドを差し込みたいとき、または配送ループを自分で制御したいときはこちら。
- **PG 自由関数** (`queue::` モジュール) —— よくあるケース向けの便利通路:すでに `sqlx::PgPool` を持っていて `queue::enqueue(pool, ...)` を呼びたいときに使う。同梱の `DeliveryWorker` と mailrs 本体がこちらを使う。

両方とも v1.x で後方互換性ありで安定。v2 計画は trait サーフェスへの統合 + ジェネリックワーカー、ただし v1 ユーザーコードを壊すつもりはない。

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
