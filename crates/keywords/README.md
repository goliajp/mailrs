# mailrs-keywords

[![Crates.io](https://img.shields.io/crates/v/mailrs-keywords?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-keywords)
[![docs.rs](https://img.shields.io/docsrs/mailrs-keywords?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-keywords)
[![License](https://img.shields.io/crates/l/mailrs-keywords?style=flat-square)](#license)

Reader and writer for the Maildir++ keyword map. A message's `:2,`
suffix carries the standard flags as uppercase letters and up to 26
keyword bits as lowercase ones; the file beside the mail says what each
bit stands for:

```text
0 archived
1 pinned
```

so `1786650987.M1P1.host:2,Sab` is seen, archived and pinned. The file
is named `mailrs-keywords` and uses the format of Dovecot's
`dovecot-keywords`, unchanged.

A name's index is its identity and the letter is derived from it
(`0` is `a`). `Keywords::intern` only ever appends, because renumbering
an existing name would silently re-point every message carrying its bit.

## Example

```rust
let dir = std::path::Path::new("/var/mail/alice");
let mut keywords = mailrs_keywords::read(dir)?;
let bit = keywords.intern("pinned").expect("all 26 keyword bits are taken");
mailrs_keywords::write(dir, &keywords)?;
assert_eq!(keywords.name(bit), Some("pinned"));
# Ok::<(), std::io::Error>(())
```

## API

- `Keywords::parse` / `Keywords::render`: the file format.
- `Keywords::letter`, `Keywords::name`, `Keywords::names_of`: look up in
  either direction.
- `Keywords::intern`: assign the next free bit to a new name (at most
  26; `None` when full).
- `path`, `read`, `write`: the file beside a mailbox directory. `write`
  replaces the file atomically.

## License

Apache-2.0 OR MIT.
