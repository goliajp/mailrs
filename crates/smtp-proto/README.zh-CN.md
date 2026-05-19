# mailrs-smtp-proto

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-proto?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-smtp-proto)
[![docs.rs](https://img.shields.io/docsrs/mailrs-smtp-proto?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-smtp-proto)
[![License](https://img.shields.io/crates/l/mailrs-smtp-proto?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-smtp-proto?style=flat-square)](https://crates.io/crates/mailrs-smtp-proto)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust 的 SMTP 协议解析器、格式化器以及会话状态机 —— 零 I/O,与异步运行时无关。

实现了 [RFC 5321] (SMTP) 以及任何真实邮件服务器都用得到的扩展:STARTTLS ([RFC 3207])、AUTH PLAIN / LOGIN ([RFC 4954]、[RFC 4616])、SMTPUTF8 ([RFC 6531])、增强状态码 ([RFC 3463]),以及 SIZE / 8BITMIME / PIPELINING。

## 亮点

- **零 I/O** —— 纯解析和状态机逻辑,不带 TCP、不带 TLS、不带异步运行时。调用方负责接线。
- **零拷贝解析** —— `parse_command()` 返回的 `Command<'_>` 直接借用输入切片。
- **完整状态机** —— `Session::handle_command()` 把命令映射到 `Event` 决策:回复 / 进 DATA / 升级 TLS / 验身份 / 关连接。
- **命名响应构造器** —— `Response::mail_ok()` / `Response::dnsbl_reject(...)` / `Response::greylisted()` 等,覆盖 RFC 5321 + 真实用到的反垃圾响应。
- **生产环境验证** —— 从 [mailrs](https://github.com/goliajp/mailrs) (一个 Rust 邮件服务器) 拆出,232 个测试,无 `unsafe`,外部依赖只有一个 ([base64])。

## 快速开始

```rust
use mailrs_smtp_proto::{parse_command, Command, Session, SessionConfig, Event};

let mut session = Session::new("smtp.example.com", SessionConfig::default());

let cmd = parse_command("EHLO client.example.org").unwrap();
assert!(matches!(cmd, Command::Ehlo("client.example.org")));

match session.handle_command(&cmd) {
    Event::Reply(resp) => {
        // 把 resp.format() 写到网络上, 然后读下一条命令
    }
    Event::NeedData { reverse_path, forward_paths } => {
        // MAIL FROM + RCPT TO + DATA 全部通过, 现在读消息体
    }
    Event::StartTls(_) => {
        // 升级 TLS 连接, 然后调 session.reset_after_tls()
    }
    Event::Shutdown(_) => {
        // 写响应, 关连接
    }
    Event::NeedAuth { username, password } => {
        // 外部验证凭证, 然后调 session.set_authenticated()
    }
    Event::AuthChallenge { response, step } => {
        // 写挑战, 读下一行, 调 session.handle_auth_response()
    }
}
```

完整 EHLO / MAIL FROM / RCPT TO / DATA 流程见 [`examples/parse_and_drive.rs`](examples/parse_and_drive.rs)。

## 这个 crate 不做什么

- 不做 I/O。不带 TCP、不带 TLS、不带异步运行时,调用方接线。
- 不存消息,不做 DKIM/SPF/DMARC。
- 不是出站 SMTP 客户端 —— 那是后续 `mailrs-smtp-client` 的事。

## 模块速览

| 模块 | 做什么 |
|------|--------|
| `command` | 类型化的 `Command<'a>` 枚举 + 载荷类型 (`ReversePath` / `ForwardPath` / `Param` / `AuthMechanism`)。 |
| `parse` | `parse_command(&str) -> Command<'_>`,覆盖 RFC 5321 所有命令以及 AUTH 和 STARTTLS。 |
| `response` | `Response` + 所有常用回复的命名构造器, 以及 `format_ehlo_response` 处理多行 EHLO。 |
| `session` | `Session` 状态机, `Command` → `Event`。跟踪 EHLO / MAIL FROM / RCPT TO / DATA / RSET / STARTTLS / AUTH 全套状态转移。 |
| `auth` | SASL 帮手:`decode_plain` (AUTH PLAIN) + `decode_login_response` (AUTH LOGIN)。 |
| `data` | `unstuff_line` / `unstuff_data` 处理 DATA 阶段的 dot-stuffing。 |
| `address` | 最小 `is_valid` / `split_address` 帮手。 |

## 为什么单独成 crate?

`mailrs-smtp-proto` 故意只做协议这一层。任何 Rust 邮件相关项目(收件 / MTA / milter / MX 测试工具)都更愿意自己控制 I/O 和身份验证策略 —— 麻烦的是 wire-format 解析、状态机角落、以及那条很长的响应码尾巴。这个 crate 就是为这个存在的。

它同时是 [mailrs] 邮件服务器入站监听器的底座。把它单独发出来意味着大家用的是同一份经过实战验证的核心。

## 许可证

双许可证, 二选一:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT 许可证 ([LICENSE-MIT](LICENSE-MIT) 或 <https://opensource.org/licenses/MIT>)

除非你明确声明,任何提交到本项目的贡献,按 Apache-2.0 的定义,都将以上述双许可证发布,没有任何附加条款。

[RFC 5321]: https://datatracker.ietf.org/doc/html/rfc5321
[RFC 3207]: https://datatracker.ietf.org/doc/html/rfc3207
[RFC 4954]: https://datatracker.ietf.org/doc/html/rfc4954
[RFC 4616]: https://datatracker.ietf.org/doc/html/rfc4616
[RFC 6531]: https://datatracker.ietf.org/doc/html/rfc6531
[RFC 3463]: https://datatracker.ietf.org/doc/html/rfc3463
[mailrs]: https://github.com/goliajp/mailrs
[base64]: https://crates.io/crates/base64
