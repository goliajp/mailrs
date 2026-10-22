# mailrs-uidlist

[![Crates.io](https://img.shields.io/crates/v/mailrs-uidlist?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-uidlist)
[![docs.rs](https://img.shields.io/docsrs/mailrs-uidlist?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-uidlist)
[![License](https://img.shields.io/crates/l/mailrs-uidlist?style=flat-square)](#license)

Reader and writer for the IMAP UID to filename map, stored beside the
mail rather than in a server's index. A UID is a promise to an IMAP
client: the number keeps meaning the same message for as long as
`UIDVALIDITY` does not change. Keeping the map next to the Maildir means
a rebuilt index does not force every client to resynchronise.

The file is named `mailrs-uidlist` and uses the format of Dovecot's
`dovecot-uidlist`, unchanged:

```text
3 V1786650000 N4213
4212 :1786650987.M1P1.host
4213 :1786650999.M2P1.host
```

A header line (format version, `V`alidity, `N`ext UID), then one record
per message: the UID, optional extension fields, `:` and the base
filename without the `:2,` flag suffix. Extension fields Dovecot writes
(`W<size>`, `G<guid>`) are read and ignored, so a Dovecot-written file
can be adopted as is.

## Example

```rust
let dir = std::path::Path::new("/var/mail/alice");
mailrs_uidlist::append(dir, 4214, "1786651002.M3P1.host")?;
if let Some(list) = mailrs_uidlist::read(dir)? {
    assert_eq!(list.uid_of("1786651002.M3P1.host"), Some(4214));
}
# Ok::<(), std::io::Error>(())
```

## API

- `UidList::parse` / `UidList::render`: the file format.
- `UidList::filename`, `UidList::uid_of`: look up in either direction.
- `UidList::compacted`: drop superseded records.
- `path`, `read`, `append`, `append_many`, `rewrite`: the file beside a
  mailbox directory.

`append` writes one record with `O_APPEND` and leaves the header alone,
so the stored `N` can go stale between rewrites. `read` derives the next
UID as `max(header N, highest UID + 1)`, so callers never see the
difference.

## License

Apache-2.0 OR MIT.
