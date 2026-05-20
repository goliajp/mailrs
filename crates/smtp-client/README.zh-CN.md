# mailrs-smtp-client

[![Crates.io](https://img.shields.io/crates/v/mailrs-smtp-client?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-smtp-client)
[![docs.rs](https://img.shields.io/docsrs/mailrs-smtp-client?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-smtp-client)
[![License](https://img.shields.io/crates/l/mailrs-smtp-client?style=flat-square)](#许可证)
[![Downloads](https://img.shields.io/crates/d/mailrs-smtp-client?style=flat-square)](https://crates.io/crates/mailrs-smtp-client)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | **简体中文** | [日本語](README.ja.md)

Rust 的出站 SMTP 客户端原语 —— MX 解析、DANE/STARTTLS、多行响应解析。

构建在 `tokio` + `rustls` + `hickory-resolver` 之上。实现 MTA 真正派件到公网邮件服务器需要的全部环节:查 MX 记录、按优先级挑服务器、建一条可被 DNSSEC 锚定 TLSA 记录验证的 TLS 连接 ([RFC 7672] DANE)、读跨行的 SMTP 响应。

## 亮点

- **带缓存的 MX 解析** —— `resolve_mx()` 返回按 preference 排好的列表;`MxCache` 让多次投递可复用结果。
- **DANE 验证** —— `resolve_tlsa()` + `DaneVerifier` 把 TLSA 绑定的证书强制到 SMTP relay 上,挡住主动降级 STARTTLS 的中间人攻击。
- **连接驱动** —— `SmtpConnection` 包装了读写循环, 每条命令独立超时 (`TimeoutConfig`), 慢服务器不会把发件端卡死。
- **响应解析** —— `parse_response()` 处理 [RFC 5321 §4.2.1] 的 `250-...` / `250 ...` 多行格式。
- **dot-stuffing** —— `dot_stuff()` 把 DATA 载荷里的行首点转义掉, 杜绝因为内嵌 `\r\n.\r\n` 导致消息被截断。
- **生产环境验证** —— 从 [mailrs] (一个 Rust 邮件服务器) 拆出。

## 快速开始

```rust,no_run
use mailrs_smtp_client::{MxCache, SmtpConnection, TokioResolver, sort_mx_records};
use std::time::Duration;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let resolver = TokioResolver::builder_tokio()?.build()?;
let cache = MxCache::new(Duration::from_secs(300)); // MX 答案缓存 5 分钟

// 1. 查收件方域名的 MX, 按 preference 排序
let mut records = cache.resolve(&resolver, "example.com").await?;
sort_mx_records(&mut records);
let primary = records.first().ok_or("no MX")?;

// 2. 建连接 (内部已读 banner) 并 EHLO
let mut conn = SmtpConnection::connect(&primary.exchange, 25).await?;
conn.ehlo("client.example.org").await?;

// 3. STARTTLS 升级、re-EHLO、投递消息
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

完整 MX 解析 + EHLO + QUIT 流程见 [`examples/resolve_and_connect.rs`](examples/resolve_and_connect.rs)。

## 这个 crate 不做什么

- 不做 DKIM 签名、SPF 检查、DMARC 对齐 —— 那些应该在上游做 (比如 [mail-auth]);本 crate 只负责 wire-level 客户端。
- 不做出站队列、重试、DSN 生成 —— 那是 `mailrs-outbound-queue` 的事。
- 不做 SMTP 服务端 —— 收件侧状态机看 [`mailrs-smtp-proto`]。

## 模块速览

| 模块 | 做什么 |
|------|--------|
| `mx` | DNS MX 查询、preference 排序、内存缓存、回退到 A 记录。 |
| `dane` | TLSA 记录解析 + 证书验证 ([RFC 7672])。 |
| `connection` | `SmtpConnection` 包装可 TLS 升级的读写循环, 带超时。 |
| `response` | 单行 / 多行 SMTP 回复解析 ([RFC 5321 §4.2.1])。 |

## 为什么单独成 crate?

大多数 "SMTP client" crate 要么打包成一个完整 MUA (auth、MIME 构建、附件), 要么只到 "TCP+EHLO+MAIL FROM"。MTA 这两个都不要:消息字节已经在手, 真正需要的是那条长尾 —— 按 preference 排好的 MX 列表、DANE 验证过的 TLS、稳健的多行回复解析、不会让一个慢远端拖死连接池的超时。这就是这个 crate 的位置。

它是 [mailrs] 邮件服务器的出站侧, 单独发出来, 任何 Rust 写 MTA / 投递自测工具 / 退信探针的项目都可以基于同一份经过实战验证的代码。

## 许可证

双许可证, 二选一:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT 许可证 ([LICENSE-MIT](LICENSE-MIT) 或 <https://opensource.org/licenses/MIT>)

除非你明确声明, 任何提交到本项目的贡献, 按 Apache-2.0 的定义, 都将以上述双许可证发布, 没有任何附加条款。

[RFC 5321 §4.2.1]: https://datatracker.ietf.org/doc/html/rfc5321#section-4.2.1
[RFC 7672]: https://datatracker.ietf.org/doc/html/rfc7672
[mail-auth]: https://crates.io/crates/mail-auth
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[mailrs]: https://github.com/goliajp/mailrs
