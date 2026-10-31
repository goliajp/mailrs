# mailrs-reputation

Says what this server has observed about where a message comes from and
where it points: the sending IP, the envelope and header sender domains,
and the hosts its links lead to. The observations are the server's own —
incidents, sign-in lockouts, complaints, mail its users moved to Junk, an
administrator's allow and deny. Nothing is looked up over the network.

The crate does not judge. It answers with facts — each listing with its
observation id, source, reason and age in days; each domain's marks with
who made them and when; whether a domain is on the never-list — and the
rules that decide what those facts are worth live elsewhere.

```rust
use mailrs_reputation::{Message, NeverList, Set};

let set = Set::build(&[], &NeverList::builtin(&[]), 0);
let facts = set.facts(&Message::default());
assert!(facts.ip.is_empty() && facts.sender_marks.is_none());
```

Observations are facts: added, never changed; a mistake is taken back by a
`Retract`. The set is derived from them and can be rebuilt at any time. An
allow always wins over a deny of the same subject or a parent of it.
