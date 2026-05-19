# mailrs-imap-proto

[![Crates.io](https://img.shields.io/crates/v/mailrs-imap-proto?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-imap-proto)
[![docs.rs](https://img.shields.io/docsrs/mailrs-imap-proto?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-imap-proto)
[![License](https://img.shields.io/crates/l/mailrs-imap-proto?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-imap-proto?style=flat-square)](https://crates.io/crates/mailrs-imap-proto)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust 的 IMAP4rev1 协议解析器、响应格式化器、序列集帮手 —— 零 I/O,与异步运行时无关。

实现了 [RFC 3501] (IMAP4rev1) 的 wire-format 部分:带 tag 的命令解析、序列集算术、SEARCH key 解析,以及最常用的 tagged + untagged 响应格式化。连接状态、邮箱存储、IDLE/AUTHENTICATE 消息泵这些都是调用方的事。

## 亮点

- **零 I/O** —— 纯解析 + 格式化。不带 TCP、不带 TLS、不带异步运行时。
- **类型化命令** —— `parse_command()` 返回 `TaggedCommand { tag, command: ImapCommand }`。`ImapCommand` 枚举覆盖 LOGIN / SELECT / FETCH / STORE / SEARCH / IDLE / APPEND / UID 前缀变体等。
- **序列集** —— `parse_sequence_set("1,3:5,7:*")` → 类型化 `SequenceSet`; `sequence_set_to_uids(&set, max)` → 排序去重的 UID 列表。处理 `*`、范围、列表、越界裁剪。
- **SEARCH key** —— `parse_search_criteria()` 返回类型化 `Vec<SearchKey>` (FROM / TO / SUBJECT / TEXT / BODY / SEEN / UNSEEN / SINCE / BEFORE / UID / ...)。
- **响应格式化器** —— `format_ok` / `format_no` / `format_bad` (tagged); `format_capability` / `format_list` / `format_fetch` / `format_flags` / `format_exists` / `format_recent` / `format_bye` / `format_quota` / `format_quotaroot` (untagged)。
- **生产环境验证** —— 从 [mailrs](https://github.com/goliajp/mailrs) (一个 Rust 邮件服务器) 拆出,225 个测试,无 `unsafe`,零外部依赖。

## 快速开始

```rust
use mailrs_imap_proto::{
    parse_command, parse_sequence_set, sequence_set_to_uids,
    format_capability, format_fetch, format_ok, ImapCommand,
};

// 解析一条带 tag 的命令行
let parsed = parse_command("a001 CAPABILITY").unwrap();
assert_eq!(parsed.tag, "a001");
assert_eq!(parsed.command, ImapCommand::Capability);

// 对 8 条消息的邮箱展开序列集
let set = parse_sequence_set("1,3:5,7:*").unwrap();
let uids = sequence_set_to_uids(&set, 8);
assert_eq!(uids, vec![1, 3, 4, 5, 7, 8]);

// 格式化几条响应
let _ = format_capability(&["IMAP4rev1", "IDLE", "AUTH=PLAIN"]);
let items = vec![
    ("FLAGS".to_string(), "(\\Seen)".to_string()),
    ("UID".to_string(), "42".to_string()),
];
let _ = format_fetch(1, &items);
let _ = format_ok("a001", "CAPABILITY completed");
```

更长的示例见 [`examples/parse_and_format.rs`](examples/parse_and_format.rs)。

## 这个 crate 不做什么

- 不做 I/O。不带 TCP、不带 TLS、不带异步运行时、不管连接管理。
- 不存邮箱、不索引消息。
- **不带会话状态机**。跟 SMTP 不一样,IMAP 的每连接状态(选中的邮箱、能力协商、待处理 IDLE / authenticate 继续、命令 literal 处理)归调用方所有。这个 crate 给你类型化命令进、格式化行出 —— 状态你自己留。

## 模块速览

| 模块 | 做什么 |
|------|--------|
| `command` | `parse_command(&str) -> TaggedCommand`,以及 `ImapCommand` / `SearchKey` / `ParseError`。 |
| `sequence` | `parse_sequence_set` / `sequence_set_to_uids`,处理 `*`、范围、列表、裁剪。 |
| `response` | `format_*` 函数,涵盖 tagged (OK/NO/BAD) 和 untagged (CAPABILITY/LIST/FETCH/...)。 |

## 许可证

双许可证, 二选一:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT 许可证 ([LICENSE-MIT](LICENSE-MIT) 或 <https://opensource.org/licenses/MIT>)

除非你明确声明,任何提交到本项目的贡献,按 Apache-2.0 的定义,都将以上述双许可证发布,没有任何附加条款。

[RFC 3501]: https://datatracker.ietf.org/doc/html/rfc3501
[mailrs]: https://github.com/goliajp/mailrs
