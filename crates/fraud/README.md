# mailrs-fraud

[![Crates.io](https://img.shields.io/crates/v/mailrs-fraud?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-fraud)
[![docs.rs](https://img.shields.io/docsrs/mailrs-fraud?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-fraud)
[![License](https://img.shields.io/crates/l/mailrs-fraud?style=flat-square)](#license)

Signals that a message is a fraud attempt rather than mail.

SPF, DKIM and DMARC say a message really came from the domain in its
`From:` header, and a fraudster who registers a domain the day before
passes all three. Measured on one company's mail server, every one of 25
messages impersonating the company that runs it passed SPF, and 18
passed DKIM and DMARC as well. What is left is the message itself: who
it claims to be from, what wrote it, and what it asks for. That is what
this crate checks.

Each signal is measured against a real corpus before it is added, and
the count is written in its documentation. Signals are scores, not
rulings, except where the measurement supports a verdict: a generated
`X-Mailer` matched 29 messages and all 29 were fraud, so it reaches the
default threshold on its own.

## Example

```rust
use mailrs_fraud::{Facts, Policy, RULE_GENERATED_MAILER, scan};

let facts = Facts {
    from: "Billing <ipdxuawesj@example.net>",
    x_mailer: Some("YBPGKV FPLPI 140.59825.59030.27158"),
    ..Facts::default()
};
let findings = scan(&facts, &Policy::default());
assert!(findings.has(RULE_GENERATED_MAILER));
println!("score {}, hold: {}", findings.score(), findings.hold_worthy());
```

## API

- `scan(&Facts, &Policy) -> Findings`: run every check over the facts
  extracted from one message.
- `Facts`: the header values, domains and attachment text a host
  extracts for the checks.
- `Policy`: the organisation's own names, its sending domains, domains
  allowed to carry its name (Slack, GitHub, Atlassian and the like) and
  the display names of its account holders. Empty fields turn the
  checks that need them off.
- `Finding` / `Findings`: rule id, `Layer` (`Transport`, `Identity`,
  `Provenance`, `Content`), score, and whether the finding is enough to
  hold the message. `Findings::score()` sums them;
  `Findings::hold_worthy()` says whether any finding holds.
- `RULE_*` and `*_SCORE` constants name each rule and its weight.
- One module per check (`brand`, `impersonation`, `mailer_fingerprint`,
  `minted_address`, `reply_rotation`, `sending_host`, `hostname_claim`,
  `greeting`, `attachment`, `advance_fee`, `subject_is_our_name`).

The `testing` feature adds `findings_for(rules)`, which builds
`Findings` by rule id for downstream tests.

## Not in scope

- Content classification (whether the words are spam): see
  `mailrs-bayes`.
- Deceptive characters in a name: see `mailrs-textguard`.
- Sender reputation. Nothing here remembers a sender; the fraud it was
  built against changes domain every few messages.

## License

Apache-2.0 OR MIT.
