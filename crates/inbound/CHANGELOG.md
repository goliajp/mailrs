# Changelog

All notable changes to `mailrs-inbound` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed (breaking)

- `fold_sender_trust` takes a fourth argument, the
  `mailrs_textguard::Deception` found in the From display name and
  Subject. A bidirectional override there makes the sender
  `Suspicious` even when DMARC passed.
- `PipelineInput` has new public fields: `deception`, `fraud`
  (`mailrs_fraud::Findings`), `header_from_addr`, `local_domains` and
  `sender_allowed`. Code that builds it with a struct literal must set
  them (`verdict::unexamined()` returns an input with nothing found).
- The recipient whitelist now matches the envelope sender or the
  `From:` address and no longer requires SPF or DKIM to pass; it is
  overruled only when DMARC failed under `p=quarantine` (or
  `p=reject`, which rejects first). The blacklist matches either
  address too.

### Added

- `verdict` module: `assess(input)` returns a `FraudVerdict` with
  `rules_version` (`RULES_VERSION`), score, whether the message is held
  (`holds`), and one `Layer` per review with an `Outcome` (`Pass`,
  `Fail`, `NotApplicable`).
- `identity` module: header readers shared by every caller
  (`from_header`, `from_address`, `reply_to_address`, `subject_header`,
  `to_display_name`, `x_mailer_header`, `first_received_peer_ip`,
  `address_is_private`, `auth_results_authserv`,
  `auth_results_tokens`, `deception_in_display_name`,
  `deception_in_identity`).
- `dates` module: `parse_date`, `date_header_epoch`, `received_epoch`
  and `date_ahead_of`, for mail dated ahead of its arrival.
- `body` module: `is_bulk`, `offers_the_reader_a_sum`,
  `attachment_offers_the_reader_a_sum`, `attachment_text_for_rules`.
- Decision rules: an operator allow list (`sender_allowed`) accepts
  ahead of DMARC and the blacklist (not ahead of a virus); mail whose
  envelope sender is in `local_domains` and passed SPF or DKIM is
  accepted; zero-width padding in the display name or Subject adds
  `UNJUSTIFIED_ZERO_WIDTH_SCORE` (2.5); fraud findings from
  `mailrs-fraud` add their scores, and the strongest of them reach the
  threshold alone.
- `sender_listed`, `ReceiveContext::with_fraud`,
  `ReceiveContext::with_local_domains`,
  `ReceiveContext::sender_trust_with`, `auth_header::sender_trust_with`.
- New dependencies: `mailrs-fraud`, `mailrs-textguard`, `mailrs-mime`,
  `mailrs-rfc2047`, `chrono`, `serde`. `mailrs-fraud` and
  `mailrs-textguard` are not on crates.io yet and must be published
  first.

## [1.2.0] - 2026-08-04

### Added

- `auth_header::SenderTrust` (`Verified`, `Unverified`, `Suspicious`)
  with `as_str`, `fold_sender_trust`, `sender_trust` and
  `parse_auth_results` for reading an `Authentication-Results:` field
  back; `AuthResults::sender_trust` for the receive-time results.
- `SUSPICIOUS_SENDER_SCORE` (3.0): a `Suspicious` sender adds this to
  the spam score. Alone it stays below the default threshold of 5.0.
- Per-recipient sender lists: `ReceiveContext` and `PipelineInput`
  gained `from_addr`, `recipient_whitelist` and `recipient_blacklist`.
  A whitelisted sender whose SPF or DKIM passed is accepted; a
  blacklisted one goes to Junk. Virus and DMARC `p=reject` still win.
- `Pipeline::run` records a `mailrs_inbound_stage_seconds` histogram
  per stage (labels `stage`, `outcome`) through the `metrics` crate.
  New dependency: `metrics`.

### Changed

- `build_auth_header` writes the header directly instead of through
  intermediate strings (about 11 times faster). Output is unchanged.

## [1.1.0] - 2026-05-23

### Added

- **Tracing instrumentation on `Pipeline::run`.** Emits one
  `info_span!("inbound.pipeline", n_stages, spam_threshold)` for the
  whole run + one nested `debug_span!("inbound.stage", name=…)` per
  evaluated stage. Per-stage futures are attached via
  `tracing::Instrument` so the spans correctly survive `.await`
  suspension. When a `tracing-subscriber` is set up and the caller is
  already inside a span (e.g. an `smtp.conn` span on the SMTP server's connection
  handler), the pipeline + stage spans nest under it automatically and
  show up as a clean tree in any OTel-compatible viewer.
- New dependency: `tracing = "0.1"`. No-op overhead when no subscriber
  is attached (~5-15 ns / stage measured; within criterion noise of the
  previous baseline at 610 ns / 4-noop-stages).

### Notes

- The `Stage` trait is unchanged. Stage implementors don't have to
  opt in — the executor wraps every `stage.evaluate()` call in a span
  using the stage's existing `name()` for the span field.
- Public API unchanged. Existing callers see no behaviour change beyond
  the new span emissions.

## [1.0.5] - 2026-05-22

### Changed

- Test budget change only; no library change.

## [1.0.4] - 2026-05-22

### Changed

- The Junk reason string is built in one pre-sized buffer, about 9%
  faster on the Junk decision path. Output is unchanged.

## [1.0.3] - 2026-05-22

### Added
- New `benches/pipeline.rs` (4 bench groups, 11 cases) covering `make_delivery_decision` and `Pipeline::run`. Headline medians: `make_delivery_decision_accept` ~337 ns, `make_delivery_decision_junk` ~671 ns (was 735 ns), `pipeline_run/4_noop_stages` ~610 ns, `pipeline_run/early_reject_short_circuit` ~201 ns.
- README `## Performance` section documenting the measured criterion medians (M-series Mac, release profile, 100-sample).

### Changed
- Junk-path `make_delivery_decision` -8.7% (735 ns → 671 ns) via pre-sized `String` + `write!` replacing `format!` + `matched_rules.join`.

## [1.0.2] - 2026-05-22

### Added
- New perf gate `pipeline_run_dispatch_overhead_under_budget`: measures `Pipeline::run` framework cost with four `NoopStage`s (async dispatch + final `make_delivery_decision` only, no real stage I/O). Budget 100 µs, observed ~3 µs.

### Changed
- `BUDGETS.md` clarifies that real-world `Pipeline::run` cost is owned by consumer stage backends; the dispatch gate guards framework-level regressions (per-stage alloc, mutex on hot path).

## [1.0.1] - 2026-05-22

### Added
- Initial perf regression gates and `BUDGETS.md`, closing the phase-5 polish gap.

## [1.0.0] - 2026-05-21

### Added
- Initial release. Composable SMTP receive pipeline framework: `Stage` trait, early-reject executor, pure decision logic, and RFC 8601 Authentication-Results helpers. Framework-only — consumers bring their own greylist, DKIM, virus-scan, and scoring stages.
