# mailrs-report

[![Crates.io](https://img.shields.io/crates/v/mailrs-report?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-report)
[![docs.rs](https://img.shields.io/docsrs/mailrs-report?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-report)
[![License](https://img.shields.io/crates/l/mailrs-report?style=flat-square)](#license)

Finds the other copies of a message someone reported as a scam, and says
whether that report may take them out of other people's inboxes.

- `fingerprint`: a 64-bit SimHash of the body after the reader's name and
  address, every digit, and every link's path and query are removed, so
  copies personalised for each recipient hash within a few bits of each
  other. Words are tokens; CJK text is cut into overlapping pairs. A body
  with fewer than 8 tokens has no fingerprint.
- `template::same`: two messages are one template when their hashes are
  at most 3 bits apart **and** they share a link domain, an attachment or
  the sender domain. A close body alone is not enough: a genuine notice
  and a scam copying its wording read alike.
- `decide`: one report always holds the reporter's copy. Without a ground the
  other copies wait for an administrator. Taking other
  copies needs a ground (the reporter is an administrator, two different
  people reported it within a day, or it hits a hold-level rule in force),
  and waits for an administrator when more than 20 inboxes would be
  touched, the sender has written to us for more than 90 days, or the
  sender is on the manual allow list.

The crate reads no store and sends nothing; the caller finds the
candidates and acts on the decision.

```rust
let body = "Dear Alice, your parcel 12345 is held. Pay the fee at https://evil.example/p?id=9 within 24 hours or it is returned to the sender.";
let h = mailrs_report::fingerprint::of_text(body, &["alice"]).unwrap();
let pieces = mailrs_report::fingerprint::segments(h);
assert_eq!(pieces.len(), 4);
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at
your option.
