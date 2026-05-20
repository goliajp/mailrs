# mailrs-ical

[![Crates.io](https://img.shields.io/crates/v/mailrs-ical?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-ical)
[![docs.rs](https://img.shields.io/docsrs/mailrs-ical?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-ical)
[![License](https://img.shields.io/crates/l/mailrs-ical?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-ical?style=flat-square)](https://crates.io/crates/mailrs-ical)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust 向け RFC 5545 (iCalendar) + RFC 5546 (iTIP) パーサー、シリアライザ、型付きセマンティクス —— 手書き、I/O ゼロ、VTIMEZONE + RRULE 対応。

[mailrs] から切り出されており、`text/calendar` 招待を取り込んだり (`REPLY` ペイロードを生成したり) する Rust プロジェクトはどれも、同じ実戦検証済みのコアに乗れる:パーサーコンビネータ非依存のバイト単位パース、型付き `Method` / `Attendee` / `Organizer` / `CalDateTime`、chrono-tz IANA フォールバック付き VTIMEZONE 処理。

## 特徴

- **I/O ゼロ** —— 純粋なパースとフォーマット。ファイルシステムも、ネットワークも、非同期ランタイムも含まない。呼び出し側が配線する。
- **型付きセマンティクス** —— [`parse_invite`] は完全に型付けされた [`ParsedInvite`] を返す (`METHOD` / `UID` / `SEQUENCE` / `DTSTAMP` / `DTSTART` / `DTEND` / `ATTENDEE` / `ORGANIZER` / `RRULE` / `EXDATE` / `RDATE` / `RECURRENCE-ID` / `STATUS` / `SUMMARY` / `LOCATION` / `DESCRIPTION` / `VTIMEZONE`)。
- **VTIMEZONE スマートフォールバック** —— RFC 5545 のインライン VTIMEZONE ブロックを受け付け、TZID が IANA の既知ロケーション名であれば chrono-tz にフォールバック。
- **iTIP 全対応** —— [`Method`] 列挙体が RFC 5546 の `REQUEST` / `REPLY` / `CANCEL` / `UPDATE` / `COUNTER` / `REFRESH` / `ADD` / `PUBLISH` / `DECLINECOUNTER` をカバー。
- **シリアライザ** —— [`serialize`] が [`ParsedInvite`] を RFC 5545 テキストに戻し、iTIP `REPLY` 本文にそのまま使える。
- **実運用で検証済み** —— Rust メールサーバーから切り出し、Outlook / Nextcloud / Google / Apple Calendar / Thunderbird の実 `.eml` コーパスでパスを確認。

## クイックスタート

```rust
use mailrs_ical::{parse_invite, Method};

let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\n\
            PRODID:-//Example//Cal//EN\r\nBEGIN:VEVENT\r\n\
            UID:abc\r\nDTSTAMP:20260101T000000Z\r\n\
            DTSTART:20260102T100000Z\r\nSUMMARY:Lunch\r\n\
            END:VEVENT\r\nEND:VCALENDAR\r\n";

let invite = parse_invite(ics).unwrap();
assert_eq!(invite.method, Method::Request);
assert_eq!(invite.uid, "abc");
assert_eq!(invite.summary, "Lunch");
```

パース + 型付きビューの表示 + シリアライズの往復は [`examples/parse_invite.rs`](examples/parse_invite.rs) を参照。

## この crate が「やらない」こと

- MIME パースはしない —— `text/calendar` パートは上流で抽出する (例: [`mail-parser`])。
- SMTP はしない —— [`mailrs-smtp-proto`] / [`mailrs-smtp-client`] を参照。
- カレンダーの永続化、CalDAV サーバーは扱わない。本 crate は wire 形式の層のみ。
- RRULE **展開** はしない —— raw RRULE 文字列をキャプチャするのみ。展開が必要な場合は [`rrule`] crate を使う。

## モジュール一覧

| モジュール   | 役割 |
|--------------|------|
| `parse`      | RFC 5545 §3.1 テキスト → raw AST (行折り、プロパティツリー、BEGIN/END 入れ子)。 |
| `semantics`  | AST → 型付き [`ParsedInvite`] (METHOD, ATTENDEE, ORGANIZER, SEQUENCE, RRULE, …)。 |
| `vtimezone`  | インライン VTIMEZONE 処理 + chrono-tz IANA フォールバック。 |
| `serialize`  | [`ParsedInvite`] → RFC 5545 テキスト (iTIP REPLY 用)。 |

## ライセンス

下記いずれかを選択:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ライセンス ([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

明示的に別段の意思表示をしない限り、本プロジェクトへの貢献は Apache-2.0 ライセンスの定義に従い、追加条件なしで上記の二重ライセンスで配布されるものとする。

[mailrs]: https://github.com/goliajp/mailrs
[`mail-parser`]: https://crates.io/crates/mail-parser
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
[`rrule`]: https://crates.io/crates/rrule
