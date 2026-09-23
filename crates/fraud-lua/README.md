# Lua fraud rules on Luna

The receiver and historical fraud rescan run all 18 predicates through
`luna-jit = 3.0.0` from **goliajp/luna**. JIT is disabled for these bounded
scripts because native loops bypass Luna's interpreter instruction budget.
The existing Rust rule functions remain a migration oracle; neither production
scoring entry point calls them. New rules and brand entries belong in `rules/`.

`brands.lua` is the brand/domain data; `identity.lua`, `provenance.lua` and
`content.lua` register predicates. `helpers.lua` supplies shared comparisons.
Rust normalizes Unicode, parses addresses and extracts message facts. Lua owns
the conditions, thresholds, scores, explanations and hold grades.

```lua
rule('brand-is-the-display-name', 'identity', 6, true, function(m)
    -- Return a nonempty explanation on a match; nil otherwise.
end)
```

A `true` hold grade enters the existing Review queue and marks the conversation
read, including the per-message rows and Maildir flags. A `false` grade only
contributes to the spam score. Lua cannot delete or release mail or perform I/O.

## Publish a rule update

After the one-time binary/compose deployment, ordinary rule changes need **no
Cargo build, image push or process restart**:

```bash
cat crates/fraud-lua/rules/{brands,helpers,identity,provenance,content}.lua > /tmp/fraud.lua
./scripts/deploy-fraud-rules.sh /tmp/fraud.lua             # validate + read-only corpus diff
./scripts/deploy-fraud-rules.sh /tmp/fraud.lua --activate  # compare again, then atomic activation
```

The script uses the checker already shipped in the running image. It serializes
publication, verifies the uploaded SHA-256, compares the current and proposed Lua
bundles over the maildir, records a report, and atomically replaces
`/apps/mailrs/fraud-rules/active.lua`. `previous.lua` is the immediate rollback.
To roll back, publish that saved bundle through the same check/activation flow.
Review the report's added/removed holds and samples before activation.

Receiver and fastcore mount the **directory** read-only, so rename is visible to
both. `MAILRS_FRAUD_RULES_FILE=/fraud-rules/active.lua` selects it. Each scoring
worker checks at most every five seconds, on its next message.

Fastcore also detects the active bundle version and queues a durable historical
backfill automatically (including the first deployment). It snapshots conversation
identities, processes 100 per batch with pauses, then checkpoints before the next
batch. A restart resumes that snapshot; new arrivals cannot shift an offset and
skip old mail. A newer bundle replaces the outstanding job. Evaluation or write
errors retain the batch cursor and retry with backoff; a fallback rule version
does not count as success for the requested version. Completed jobs do no more
mailbox scanning until the version changes.

Progress is stored in `/data/kevy-fastcore/fraud-backfill/progress.json` and logged
as `fraud backfill progress` (cursor, total, held, released, no_file, complete).
`no_file` reports conversations whose original message cannot be read; they are
left unchanged. Existing holds unsupported by the new rules are released; hold
matches enter Review and become read. A conversation nothing holds, whose fraud
score alone reaches the Junk threshold (5), is moved to Junk — the same sum the
receive path uses, so a scored rule also reaches mail delivered before it
existed; `junked` counts them. Replaying a batch only writes differences.
The manual bounded `maintenance:fraud-rescan` remains available for a dry run.

A verdict stores `lua:<SHA-256>` in its existing `rules_version` field. Compile
failures keep that worker's current VM. An evaluation failure runs the previous
working bundle (embedded Lua on a fresh worker), retaining successful findings
without double scoring. If both fail, receipt returns temporary SMTP 451 and a
rescan leaves the thread unchanged. Errors are logged; they never mean clean mail.

## Verify

```bash
cargo test -p mailrs-fraud-lua
cargo run -p mailrs-fraud-lua --bin mailrs-fraud-check -- validate /tmp/fraud.lua
# First deployment: full Rust-vs-Lua comparison, read-only data mount before roll.
VERIFY_FRAUD_MIGRATION=1 SKIP_WEB=1 ./scripts/direct-deploy.sh VERSION
```

The checker refuses a partial corpus beyond the explicit message bound and
refuses to omit oversized/unreadable messages silently. It reconstructs sender
history identically for both engines and reads account names from the same kevy
set as the receiver. This is a comparison of rule behavior on identical facts,
not a precision/recall label for every mail in the corpus.

Per rule: 200,000 interpreter instructions, rearmed before every call; 16 MiB
approximate Luna heap cap. Source is limited to 256 KiB, 64 registered rules,
100-character ids and 2 KiB finding details. No `io`, `os`, `package`, bytecode,
dynamic loading or protected calls. Per-message tables are unpinned and garbage
collected. Tests cover all old sender fixtures, every rule's positive case,
50,000 successive evaluations, isolated exceptions/loops and reload failure.
