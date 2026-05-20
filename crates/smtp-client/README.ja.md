# mailrs-smtp-client

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-client?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-smtp-client)
[![docs.rs](https://img.shields.io/docsrs/mailrs-smtp-client?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-smtp-client)
[![License](https://img.shields.io/crates/l/mailrs-smtp-client?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-smtp-client?style=flat-square)](https://crates.io/crates/mailrs-smtp-client)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust 向け 送信 SMTP クライアントの基本要素 —— MX 解決、DANE/STARTTLS、複数行応答パース。

`tokio` + `rustls` + `hickory-resolver` の上に構築。MTA が公開インターネット上の SMTP サーバーへ実際に配送するときに必要なピースを提供:MX レコードの参照、優先度順のサーバー選択、DNSSEC で固定された TLSA レコード ([RFC 7672] DANE) と突き合わせて検証可能な TLS 接続、複数行にわたる SMTP 応答の読み取り。

## 特徴

- **キャッシュ付き MX 解決** —— `resolve_mx()` は preference 順のリストを返し、`MxCache` で配送間の結果を再利用できる。
- **DANE 検証** —— `resolve_tlsa()` + `DaneVerifier` で TLSA バインドされた証明書を SMTP リレーに強制し、STARTTLS をダウングレードする能動的 MITM を防ぐ。
- **接続ドライバ** —— `SmtpConnection` が読み書きループをラップし、コマンドごとに設定可能なタイムアウト (`TimeoutConfig`) を提供。遅いサーバーが送信側を固めることがない。
- **応答パーサー** —— `parse_response()` が [RFC 5321 §4.2.1] の `250-...` / `250 ...` 複数行形式を処理。
- **ドットスタッフィング** —— `dot_stuff()` が DATA ペイロードの行頭ドットをエスケープし、`\r\n.\r\n` がメッセージを途中で打ち切らないようにする。
- **実運用で検証済み** —— Rust メールサーバー [mailrs] から切り出した。

## クイックスタート

```rust,no_run
use mailrs_smtp_client::{MxCache, SmtpConnection, TokioResolver, sort_mx_records};
use std::time::Duration;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = TokioResolver::builder_tokio()?.build()?;
let cache = MxCache::new(Duration::from_secs(300)); // MX 応答を 5 分キャッシュ

// 1. 受取側ドメインの MX を解決し、preference 順にソート
let mut records = cache.resolve(&resolver, "example.com").await?;
sort_mx_records(&mut records);
let primary = records.first().ok_or("no MX")?;

// 2. 接続 (banner は内部で読まれる) と EHLO
let mut conn = SmtpConnection::connect(&primary.exchange, 25).await?;
conn.ehlo("client.example.org").await?;

// 3. STARTTLS でアップグレード、再 EHLO、メッセージ送信
let mut conn = conn.starttls(&primary.exchange).await?;
conn.ehlo("client.example.org").await?;
conn.deliver(
    "sender@example.org",
    &["bob@example.com"],
    b"Subject: hi\r\n\r\nhello\r\n",
).await?;
conn.quit().await?;
# Ok(()) }
```

MX 解決 + EHLO + QUIT のエンドツーエンド例は [`examples/resolve_and_connect.rs`](examples/resolve_and_connect.rs) を参照。

## この crate が「やらない」こと

- DKIM 署名、SPF チェック、DMARC アライメントはしない —— それらは上流 ([mail-auth] など) で。本 crate は wire レベルのクライアントのみ。
- 送信キュー、リトライ、DSN 生成もしない —— それは `mailrs-outbound-queue` の役割。
- SMTP サーバーではない —— 受信側状態機械は [`mailrs-smtp-proto`] を参照。

## モジュール概要

| モジュール | 役割 |
|-----------|------|
| `mx` | DNS MX クエリ、preference ソート、メモリキャッシュ、A レコードへのフォールバック。 |
| `dane` | TLSA レコード解決 + 証明書検証 ([RFC 7672])。 |
| `connection` | `SmtpConnection` が TLS アップグレード可能な読み書きループをタイムアウトと共にラップ。 |
| `response` | 単一行・複数行 SMTP 応答のパース ([RFC 5321 §4.2.1])。 |

## なぜ独立 crate に?

ほとんどの「SMTP client」crate は完全な MUA (auth / MIME 構築 / 添付) を含むか、「TCP+EHLO+MAIL FROM」止まりかのどちらか。MTA はどちらも要らない:メッセージのバイト列は既に手元にあって、本当に必要なのは長い尾 —— preference 順の MX リスト、DANE 検証済み TLS、堅牢な複数行応答パース、遅いリモートが接続プールを塞がないタイムアウト。この crate はそこにいる。

これは [mailrs] メールサーバーの送信側であり、独立公開することで、Rust で MTA / 配送セルフテスト / バウンスプローブを書くプロジェクトが同じ実戦検証済みコードに乗れるようになる。

## ライセンス

下記いずれかを選択:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ライセンス ([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

明示的に別段の意思表示をしない限り、本プロジェクトへの貢献は Apache-2.0 ライセンスの定義に従い、追加条件なしで上記の二重ライセンスで配布されるものとする。

[RFC 5321 §4.2.1]: https://datatracker.ietf.org/doc/html/rfc5321#section-4.2.1
[RFC 7672]: https://datatracker.ietf.org/doc/html/rfc7672
[mail-auth]: https://crates.io/crates/mail-auth
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[mailrs]: https://github.com/goliajp/mailrs
