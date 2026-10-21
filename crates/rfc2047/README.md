# mailrs-rfc2047

[![Crates.io](https://img.shields.io/crates/v/mailrs-rfc2047?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-rfc2047)
[![docs.rs](https://img.shields.io/docsrs/mailrs-rfc2047?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-rfc2047)
[![License](https://img.shields.io/crates/l/mailrs-rfc2047?style=flat-square)](#license)

RFC 2047 MIME encoded-word decoder. Decodes `=?charset?(B|Q)?text?=`
header values (Subject, From display name, …) into UTF-8.

Supports the full WHATWG Encoding charset set via `encoding_rs`:
UTF-8, ISO-8859-*, Windows-125*, ISO-2022-JP, Shift_JIS, EUC-JP,
EUC-KR, Big5, GB18030, etc. Unknown charsets fall through to a
lossy UTF-8 pass.

## Quickstart

```rust
use mailrs_rfc2047::decode;

// ASCII inputs are returned borrowed — no allocation.
assert_eq!(decode(b"plain text"), "plain text");

// Base64 encoded UTF-8.
assert_eq!(decode(b"=?UTF-8?B?VGVzdA==?="), "Test");

// Q (quoted-printable) encoded.
assert_eq!(decode(b"=?UTF-8?Q?Hello_World?="), "Hello World");

// ISO-2022-JP (Japanese subject from real-world mail).
assert_eq!(
    decode(b"=?ISO-2022-JP?B?GyRCJDMkcyRLJEEkTxsoQg==?="),
    "こんにちは",
);

// Adjacent encoded-words collapse whitespace per RFC 2047 §6.2.
assert_eq!(
    decode(b"=?UTF-8?B?aGVsbG8=?= =?UTF-8?B?d29ybGQ=?="),
    "helloworld",
);
```

## Pairing with `mailrs-rfc5322`

This crate is the typical companion of [`mailrs-rfc5322`](https://crates.io/crates/mailrs-rfc5322).
`mailrs-rfc5322::Message::header()` returns raw header bytes; pass those
bytes to `mailrs_rfc2047::decode()` to get the decoded text:

```rust,ignore
// (this example uses the `mailrs-rfc5322` crate as well; both ship
// independently. add both to your Cargo.toml to compile.)
use mailrs_rfc5322::Message;
use mailrs_rfc2047::decode;

fn extract_subject(msg_bytes: &[u8]) -> Option<String> {
    let m = Message::new(msg_bytes);
    let subject = m.header("Subject").map(|b| decode(b).into_owned())?;
    Some(subject)
}
```

## What this crate is not

- **Not** an RFC 5322 parser. Use `mailrs-rfc5322` for that.
- **Not** a MIME body decoder (multipart, Content-Transfer-Encoding).
  This only decodes encoded-words in headers.
- **Not** a charset detector. The charset is taken verbatim from the
  encoded-word token; if a message claims `=?UTF-8?Q?…?=` and the
  bytes are actually Shift_JIS, you get garbage.

## Performance

Measured numbers in [BUDGETS.md](BUDGETS.md). Reproduce via
`cargo bench -p mailrs-rfc2047 --bench decode`.

Headline: plain-ASCII inputs return as borrowed `Cow::Borrowed(&str)`
with zero allocations and constant time (just a forward scan for
`=?`). Encoded inputs go through one `String` allocation sized to the
input length.

## License

Apache-2.0 OR MIT.
