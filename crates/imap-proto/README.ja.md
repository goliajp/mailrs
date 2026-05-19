# mailrs-imap-proto

[![Crates.io](https://img.shields.io/crates/v/mailrs-imap-proto?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-imap-proto)
[![docs.rs](https://img.shields.io/docsrs/mailrs-imap-proto?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-imap-proto)
[![License](https://img.shields.io/crates/l/mailrs-imap-proto?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-imap-proto?style=flat-square)](https://crates.io/crates/mailrs-imap-proto)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust 向け IMAP4rev1 プロトコルのパーサー・レスポンスフォーマッター・シーケンスセットヘルパー —— I/O ゼロ、非同期ランタイム非依存。

[RFC 3501] (IMAP4rev1) のワイヤーフォーマット部分を実装:タグ付きコマンドのパース、シーケンスセットの算術、SEARCH キーのパース、よく使うタグ付き + 非タグ付きレスポンスフォーマッタ。接続状態、メールボックス保管、IDLE/AUTHENTICATE のメッセージポンプは呼び出し側の責務。

## 特徴

- **I/O ゼロ** —— パースとフォーマットのみ。TCP も TLS も非同期ランタイムも含まない。
- **型付きコマンド** —— `parse_command()` は `TaggedCommand { tag, command: ImapCommand }` を返す。`ImapCommand` は LOGIN / SELECT / FETCH / STORE / SEARCH / IDLE / APPEND / UID 接頭辞のバリアント等を網羅。
- **シーケンスセット** —— `parse_sequence_set("1,3:5,7:*")` → 型付き `SequenceSet`; `sequence_set_to_uids(&set, max)` → ソート済み・重複除去済みの UID リスト。`*`・範囲・リスト・範囲外クランプを処理。
- **SEARCH キー** —— `parse_search_criteria()` は型付き `Vec<SearchKey>` (FROM / TO / SUBJECT / TEXT / BODY / SEEN / UNSEEN / SINCE / BEFORE / UID / ...) を返す。
- **レスポンスフォーマッタ** —— `format_ok` / `format_no` / `format_bad` (タグ付き); `format_capability` / `format_list` / `format_fetch` / `format_flags` / `format_exists` / `format_recent` / `format_bye` / `format_quota` / `format_quotaroot` (非タグ付き)。
- **実運用で検証済み** —— Rust メールサーバー [mailrs](https://github.com/goliajp/mailrs) から切り出し。テスト 225 件、`unsafe` なし、外部依存ゼロ。

## クイックスタート

```rust
use mailrs_imap_proto::{
    parse_command, parse_sequence_set, sequence_set_to_uids,
    format_capability, format_fetch, format_ok, ImapCommand,
};

// タグ付きコマンドをパース
let parsed = parse_command("a001 CAPABILITY").unwrap();
assert_eq!(parsed.tag, "a001");
assert_eq!(parsed.command, ImapCommand::Capability);

// 8 通のメールボックスに対してシーケンスセットを展開
let set = parse_sequence_set("1,3:5,7:*").unwrap();
let uids = sequence_set_to_uids(&set, 8);
assert_eq!(uids, vec![1, 3, 4, 5, 7, 8]);

// いくつかレスポンスを整形
let _ = format_capability(&["IMAP4rev1", "IDLE", "AUTH=PLAIN"]);
let items = vec![
    ("FLAGS".to_string(), "(\\Seen)".to_string()),
    ("UID".to_string(), "42".to_string()),
];
let _ = format_fetch(1, &items);
let _ = format_ok("a001", "CAPABILITY completed");
```

詳しい例は [`examples/parse_and_format.rs`](examples/parse_and_format.rs) を参照。

## この crate が「やらない」こと

- I/O は一切しない。TCP も TLS も非同期ランタイムも含まない。接続管理もしない。
- メールボックスの保存もメッセージインデックスもしない。
- **セッション状態機械は持たない。** SMTP とは違い、IMAP の接続単位の状態(選択中のメールボックス、能力ネゴシエーション、保留中の IDLE / authenticate 継続、コマンドリテラル処理)は呼び出し側のもの。この crate は「型付きコマンド入力 / 整形済み行出力」だけを担う。

## モジュール概要

| モジュール | 役割 |
|-----------|------|
| `command` | `parse_command(&str) -> TaggedCommand` と `ImapCommand` / `SearchKey` / `ParseError`。 |
| `sequence` | `parse_sequence_set` / `sequence_set_to_uids`。`*`・範囲・リスト・クランプ対応。 |
| `response` | `format_*` 関数。タグ付き (OK/NO/BAD) と非タグ付き (CAPABILITY/LIST/FETCH/...) を網羅。 |

## ライセンス

下記いずれかを選択:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ライセンス ([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

明示的に別段の意思表示をしない限り、本プロジェクトへの貢献は Apache-2.0 ライセンスの定義に従い、追加条件なしで上記の二重ライセンスで配布されるものとする。

[RFC 3501]: https://datatracker.ietf.org/doc/html/rfc3501
[mailrs]: https://github.com/goliajp/mailrs
