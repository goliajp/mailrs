# mailrs-maildir

[![Crates.io](https://img.shields.io/crates/v/mailrs-maildir?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-maildir)
[![docs.rs](https://img.shields.io/docsrs/mailrs-maildir?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-maildir)
[![License](https://img.shields.io/crates/l/mailrs-maildir?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-maildir?style=flat-square)](https://crates.io/crates/mailrs-maildir)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust 的 Maildir 文件系统格式底层原语 —— 原子投递、目录扫描、flag 解析。不带任何协议层。

实现 Daniel J. Bernstein 为 qmail 发明的 [Maildir] 约定,被 Dovecot / Courier / mutt / neomutt / postfix 等大部分 Unix MUA 采用。每条消息一个文件,落在 `<root>/{tmp,new,cur}/` 下,文件名编码全局唯一 ID + 可选的 flag 后缀。

## 亮点

- **原子投递** —— `deliver()` 写到 `tmp/` 后 fsync,再 rename 到 `new/`。Maildir 标志性的可靠投递技术:`new/` 里永远不会出现写到一半的消息。
- **目录扫描** —— `scan_new()` / `scan_cur()` 列出每阶段的消息和已解析的 flag。
- **文件名语法** —— `parse_flags` / `serialize_flags` / `add_flag` 处理 `":2,FLAGS"` 后缀约定。
- **崩溃安全清扫** —— `cleanup_tmp(max_age)` 清掉崩溃进程留下的过期半成品。
- **生产环境验证** —— 从 [mailrs](https://github.com/goliajp/mailrs) (一个 Rust 邮件服务器) 拆出,71 个测试,无 `unsafe`,外部依赖只有一个 ([hostname])。

## 快速开始

```rust
use mailrs_maildir::{Maildir, Flag, serialize_flags};

let md = Maildir::create("/var/mail/alice/INBOX")?;

// 投递: tmp/ → fsync → rename 到 new/
let id = md.deliver(b"From: a@example.com\r\nSubject: hi\r\n\r\nhello\r\n")?;

// 扫描
for entry in md.scan_new()? {
    println!("{} flags={:?}", entry.id, entry.flags);
}

// 客户端读完消息后: 自己 rename new/ → cur/ 并加 Seen flag
let _suffix = serialize_flags(&[Flag::Seen]);  // ":2,S"
# Ok::<(), std::io::Error>(())
```

可运行的完整例子见 [`examples/deliver_and_scan.rs`](examples/deliver_and_scan.rs)。

## 这个 crate 不做什么

- **不带 IMAP / POP3 协议层**。那是 `mailrs-imap-proto` 的事。
- **不带邮箱数据库 / UID 索引**。`cur/` vs `new/` 的二态是唯一持久化状态。任何更丰富的东西(序列号、线程、全文搜索)都在更上一层。
- **不加锁**。Maildir 设计上就是 lock-free:用原子 rename 做投递和阶段转移。

## Maildir 速览

```
<root>/
├── tmp/    # 投递中(正在写)
├── new/    # 已投递, 还没被任何客户端看到
└── cur/    # 至少被一个客户端看过; flag 写在文件名后缀里
```

文件名长这样:`1684500000.M123456P9999Q0.hostname:2,S` —— 时间戳 + 唯一性组件 + `:2,FLAGS` 后缀。这个 crate 负责解析 + 原子转移,消息本身怎么处理是你的事。

## 许可证

双许可证, 二选一:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT 许可证 ([LICENSE-MIT](LICENSE-MIT) 或 <https://opensource.org/licenses/MIT>)

除非你明确声明,任何提交到本项目的贡献,按 Apache-2.0 的定义,都将以上述双许可证发布,没有任何附加条款。

[Maildir]: https://cr.yp.to/proto/maildir.html
[mailrs]: https://github.com/goliajp/mailrs
[hostname]: https://crates.io/crates/hostname
