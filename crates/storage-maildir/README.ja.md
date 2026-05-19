# mailrs-maildir

[![Crates.io](https://img.shields.io/crates/v/mailrs-maildir?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-maildir)
[![docs.rs](https://img.shields.io/docsrs/mailrs-maildir?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-maildir)
[![License](https://img.shields.io/crates/l/mailrs-maildir?style=flat-square)](#ライセンス)
[![Downloads](https://img.shields.io/crates/d/mailrs-maildir?style=flat-square)](https://crates.io/crates/mailrs-maildir)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

Rust 向け Maildir ファイルシステム形式のプリミティブ —— 原子配送、ディレクトリスキャン、フラグ解析。プロトコル層は含まない。

Daniel J. Bernstein が qmail のために考案した [Maildir] 規約 を実装。Dovecot / Courier / mutt / neomutt / postfix など、ほとんどの Unix MUA で採用されている。メッセージはメッセージごとに 1 ファイルとして `<root>/{tmp,new,cur}/` 配下に保存され、ファイル名はグローバルに一意な ID + 任意のフラグ接尾辞をエンコードする。

## 特徴

- **原子配送** —— `deliver()` が `tmp/` に書いて fsync し、`new/` に rename。Maildir 伝統の信頼配送手法:書き途中のメッセージが `new/` に現れることは決してない。
- **ディレクトリスキャン** —— `scan_new()` / `scan_cur()` が各段階のメッセージとパース済みフラグを返す。
- **ファイル名文法** —— `parse_flags` / `serialize_flags` / `add_flag` が `":2,FLAGS"` 接尾辞規約を処理。
- **クラッシュセーフな清掃** —— `cleanup_tmp(max_age)` がクラッシュしたプロセス由来の古い半端ファイルを除去。
- **実運用で検証済み** —— Rust メールサーバー [mailrs](https://github.com/goliajp/mailrs) から切り出し。テスト 71 件、`unsafe` なし、外部依存は [hostname] のみ。

## クイックスタート

```rust
use mailrs_maildir::{Maildir, Flag, serialize_flags};

let md = Maildir::create("/var/mail/alice/INBOX")?;

// 配送: tmp/ → fsync → new/ に rename
let id = md.deliver(b"From: a@example.com\r\nSubject: hi\r\n\r\nhello\r\n")?;

// スキャン
for entry in md.scan_new()? {
    println!("{} flags={:?}", entry.id, entry.flags);
}

// クライアントが読了後、自分で new/ → cur/ に rename し Seen フラグを付ける
let _suffix = serialize_flags(&[Flag::Seen]);  // ":2,S"
# Ok::<(), std::io::Error>(())
```

実行可能な完全例は [`examples/deliver_and_scan.rs`](examples/deliver_and_scan.rs) を参照。

## この crate が「やらない」こと

- **IMAP / POP3 プロトコル層は持たない。** それは `mailrs-imap-proto` の責務。
- **メールボックス DB / UID インデックスは持たない。** `cur/` と `new/` の二状態だけが永続化状態。シーケンス番号 / スレッド / 全文検索などはひとつ上の層。
- **ロックは取らない。** Maildir は lock-free 設計:配送も段階遷移も原子 rename で実現する。

## Maildir 早見表

```
<root>/
├── tmp/    # 配送中(書き込み中)
├── new/    # 配送済み、まだどのクライアントも見ていない
└── cur/    # 少なくとも 1 クライアントが見た;フラグはファイル名接尾辞に
```

ファイル名は `1684500000.M123456P9999Q0.hostname:2,S` の形 —— タイムスタンプ + 一意性コンポーネント + `:2,FLAGS` 接尾辞。この crate はパースと原子遷移を担当し、メッセージ自体をどう扱うかは利用者次第。

## ライセンス

下記いずれかを選択:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT ライセンス ([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

明示的に別段の意思表示をしない限り、本プロジェクトへの貢献は Apache-2.0 ライセンスの定義に従い、追加条件なしで上記の二重ライセンスで配布されるものとする。

[Maildir]: https://cr.yp.to/proto/maildir.html
[mailrs]: https://github.com/goliajp/mailrs
[hostname]: https://crates.io/crates/hostname
