# mailrs-smtp-proto

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-proto?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-smtp-proto)
[![docs.rs](https://img.shields.io/docsrs/mailrs-smtp-proto?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-smtp-proto)
[![License](https://img.shields.io/crates/l/mailrs-smtp-proto?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-smtp-proto?style=flat-square)](https://crates.io/crates/mailrs-smtp-proto)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust 向け SMTP プロトコルのパーサー・フォーマッター・セッション状態機械 —— I/O ゼロ、非同期ランタイム非依存。

[RFC 5321] (SMTP) と、実運用のメールサーバーなら必ず使う拡張一式に対応しています:STARTTLS ([RFC 3207])、AUTH PLAIN / LOGIN ([RFC 4954]、[RFC 4616])、SMTPUTF8 ([RFC 6531])、拡張ステータスコード ([RFC 3463])、SIZE / 8BITMIME / PIPELINING。

## 特徴

- **I/O ゼロ** —— パースと状態機械のみ。TCP も TLS も非同期ランタイムも含まない。呼び出し側が配線する。
- **ゼロコピー パース** —— `parse_command()` は入力スライスを借用した `Command<'_>` を返す。
- **完全な状態機械** —— `Session::handle_command()` がコマンドを `Event` (応答 / DATA 開始 / TLS アップグレード / 認証 / 切断) にマッピング。
- **名前付きレスポンスコンストラクタ** —— `Response::mail_ok()` / `Response::dnsbl_reject(...)` / `Response::greylisted()` など、RFC 5321 と実運用で必要なアンチスパム応答を一通り。
- **実運用で検証済み** —— Rust メールサーバー [mailrs](https://github.com/goliajp/mailrs) から切り出した。テスト 232 件、`unsafe` なし、外部依存は [base64] のみ。

## クイックスタート

```rust
use mailrs_smtp_proto::{parse_command, Command, Session, SessionConfig, Event};

let mut session = Session::new("smtp.example.com", SessionConfig::default());

let cmd = parse_command("EHLO client.example.org").unwrap();
assert!(matches!(cmd, Command::Ehlo("client.example.org")));

match session.handle_command(&cmd) {
    Event::Reply(resp) => {
        // resp.format() を回線に書き出し、次のコマンドを読む
    }
    Event::NeedData { reverse_path, forward_paths } => {
        // MAIL FROM + RCPT TO + DATA まで通った。メッセージ本文を読み込む
    }
    Event::StartTls(_) => {
        // TLS アップグレード後、session.reset_after_tls() を呼ぶ
    }
    Event::Shutdown(_) => {
        // 応答を書いて接続を閉じる
    }
    Event::NeedAuth { username, password } => {
        // 外部で資格情報を検証し、session.set_authenticated() を呼ぶ
    }
    Event::AuthChallenge { response, step } => {
        // チャレンジを書き、次の行を読み、session.handle_auth_response() を呼ぶ
    }
}
```

EHLO / MAIL FROM / RCPT TO / DATA のエンドツーエンド例は [`examples/parse_and_drive.rs`](examples/parse_and_drive.rs) を参照。

## この crate が「やらない」こと

- I/O は一切しない。TCP も TLS も非同期ランタイムも含まない。呼び出し側が配線する。
- メッセージの保存はしない。DKIM/SPF/DMARC も含まない。
- 送信 SMTP クライアントではない。それは別 crate `mailrs-smtp-client` の役割。

## モジュール概要

| モジュール | 役割 |
|-----------|------|
| `command` | 型付き `Command<'a>` 列挙体とペイロード型 (`ReversePath` / `ForwardPath` / `Param` / `AuthMechanism`)。 |
| `parse` | `parse_command(&str) -> Command<'_>`。RFC 5321 全コマンドと AUTH / STARTTLS を扱う。 |
| `response` | `Response` と全主要応答の名前付きコンストラクタ。複数行 EHLO 用に `format_ehlo_response` も。 |
| `session` | `Session` 状態機械。`Command` → `Event`。EHLO / MAIL FROM / RCPT TO / DATA / RSET / STARTTLS / AUTH の遷移を追跡。 |
| `auth` | SASL ヘルパー:`decode_plain` (AUTH PLAIN) と `decode_login_response` (AUTH LOGIN)。 |
| `data` | `unstuff_line` / `unstuff_data` が DATA 段階のドットスタッフィングを処理。 |
| `address` | 最小限の `is_valid` / `split_address` ヘルパー。 |

## なぜ独立 crate に?

`mailrs-smtp-proto` は意図的にプロトコル層だけを担う。Rust でメール関連 (受信 / MTA / milter / MX テストツール) を作る場合、I/O と認証ポリシーは自分で持ちたいことがほとんどで、辛いのは wire 形式のパース、状態機械の隅、応答コードの長い裾野。この crate はそこを引き受ける。

同時に、メールサーバー [mailrs] の受信リスナーの基盤でもある。独立公開することで、誰もが同じ実戦検証済みコアを使えるようになる。

## ライセンス

下記いずれかを選択:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ライセンス ([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

明示的に別段の意思表示をしない限り、本プロジェクトへの貢献は Apache-2.0 ライセンスの定義に従い、追加条件なしで上記の二重ライセンスで配布されるものとする。

[RFC 5321]: https://datatracker.ietf.org/doc/html/rfc5321
[RFC 3207]: https://datatracker.ietf.org/doc/html/rfc3207
[RFC 4954]: https://datatracker.ietf.org/doc/html/rfc4954
[RFC 4616]: https://datatracker.ietf.org/doc/html/rfc4616
[RFC 6531]: https://datatracker.ietf.org/doc/html/rfc6531
[RFC 3463]: https://datatracker.ietf.org/doc/html/rfc3463
[mailrs]: https://github.com/goliajp/mailrs
[base64]: https://crates.io/crates/base64
