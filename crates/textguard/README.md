# mailrs-textguard

[![Crates.io](https://img.shields.io/crates/v/mailrs-textguard?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-textguard)
[![docs.rs](https://img.shields.io/docsrs/mailrs-textguard?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-textguard)
[![License](https://img.shields.io/crates/l/mailrs-textguard?style=flat-square)](#license)

Detects characters placed in a sender's display name or a subject to
deceive the reader: bidirectional overrides and zero-width padding. It
leaves alone the invisible characters real typography needs: the
zero-width joiner (emoji sequences, Indic conjuncts), the zero-width
non-joiner (required for Persian and Hindi shaping), the byte-order mark
and the soft hyphen.

Measured over 33,602 messages, on From display names and subjects only:

| class | messages | of which phishing |
|---|---:|---|
| bidi overrides | 5 (0.015%) | 5 |
| unjustified zero-width | 40 (0.119%) | 39 |
| legitimate invisibles | 59 (0.176%) | never flagged |

The check needs no brand list and does not depend on language or topic.
It is meant for identity as displayed, not for message bodies, where
invisible characters have many innocent sources. Homoglyphs are out of
scope.

## API

```rust
let d = mailrs_textguard::deception_in("\u{202E}DRAC NOSIAS");
assert!(d.any());
```

- `deception_in(&str) -> Deception`: what was found in one string.
- `deception_in_any(iter)`: the same over several strings, such as a
  display name and a subject.
- `Deception::any()`: whether anything was found.

## License

Apache-2.0 OR MIT.
