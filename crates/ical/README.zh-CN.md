# mailrs-ical

[![Crates.io](https://img.shields.io/crates/v/mailrs-ical?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-ical)
[![docs.rs](https://img.shields.io/docsrs/mailrs-ical?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-ical)
[![License](https://img.shields.io/crates/l/mailrs-ical?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-ical?style=flat-square)](https://crates.io/crates/mailrs-ical)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust 的 RFC 5545 (iCalendar) + RFC 5546 (iTIP) 解析器、序列化器和类型化语义 —— 手写、零 I/O, 带 VTIMEZONE + RRULE 支持。

从 [mailrs] 抽出, 任何需要消费 `text/calendar` 邀请 (或生成 `REPLY` 载荷) 的 Rust 项目都可以基于同一份经过实战验证的核心:字节级解析无解析器组合子依赖、类型化的 `Method` / `Attendee` / `Organizer` / `CalDateTime`、内联 VTIMEZONE 处理 (chrono-tz IANA 回退)。

## 亮点

- **零 I/O** —— 纯解析和格式化。不带文件系统、不带网络、不带异步运行时。调用方接线。
- **类型化语义** —— [`parse_invite`] 返回完整类型化的 [`ParsedInvite`], 包含 `METHOD` / `UID` / `SEQUENCE` / `DTSTAMP` / `DTSTART` / `DTEND` / `ATTENDEE` / `ORGANIZER` / `RRULE` / `EXDATE` / `RDATE` / `RECURRENCE-ID` / `STATUS` / `SUMMARY` / `LOCATION` / `DESCRIPTION` / `VTIMEZONE`。
- **VTIMEZONE 智能回退** —— 按 RFC 5545 接受内联 VTIMEZONE 块;TZID 是已知 IANA 位置名时回退到 chrono-tz。
- **iTIP 全套** —— [`Method`] 枚举覆盖 RFC 5546 的 `REQUEST` / `REPLY` / `CANCEL` / `UPDATE` / `COUNTER` / `REFRESH` / `ADD` / `PUBLISH` / `DECLINECOUNTER`。
- **序列化** —— [`serialize`] 把 [`ParsedInvite`] 转回 RFC 5545 文本, 可直接放进 iTIP `REPLY` 正文。
- **生产环境验证** —— 从一个 Rust 邮件服务器拆出;真实 `.eml` 语料库 (Outlook / Nextcloud / Google / Apple Calendar / Thunderbird) 都过。

## 快速开始

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

解析 + 打印类型化视图 + serialize 往返见 [`examples/parse_invite.rs`](examples/parse_invite.rs)。

## 这个 crate 不做什么

- 不做 MIME 解析 —— 上游抽 `text/calendar` 部分 (比如用 [`mail-parser`])。
- 不做 SMTP —— 见 [`mailrs-smtp-proto`] / [`mailrs-smtp-client`]。
- 不做日历存储, 不做 CalDAV 服务端。本 crate 只负责 wire-format 层。
- 不做 RRULE **展开** —— 只捕获 raw RRULE 字符串, 消费者需要展开时用 [`rrule`] crate。

## 模块速览

| 模块         | 做什么 |
|--------------|--------|
| `parse`      | RFC 5545 §3.1 文本 → raw AST (行折叠、属性树、BEGIN/END 嵌套)。 |
| `semantics`  | AST → 类型化 [`ParsedInvite`] (METHOD, ATTENDEE, ORGANIZER, SEQUENCE, RRULE, …)。 |
| `vtimezone`  | 内联 VTIMEZONE 处理 + chrono-tz IANA 回退。 |
| `serialize`  | [`ParsedInvite`] → RFC 5545 文本 (iTIP REPLY 用)。 |

## 许可证

双许可证, 二选一:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT 许可证 ([LICENSE-MIT](LICENSE-MIT) 或 <https://opensource.org/licenses/MIT>)

除非你明确声明, 任何提交到本项目的贡献, 按 Apache-2.0 的定义, 都将以上述双许可证发布, 没有任何附加条款。

[mailrs]: https://github.com/goliajp/mailrs
[`mail-parser`]: https://crates.io/crates/mail-parser
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
[`rrule`]: https://crates.io/crates/rrule
