# Changelog

All notable changes to `mailrs-inbound` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.0] - 2026-05-23

### Added

- **Tracing instrumentation on `Pipeline::run`.** Emits one
  `info_span!("inbound.pipeline", n_stages, spam_threshold)` for the
  whole run + one nested `debug_span!("inbound.stage", name=…)` per
  evaluated stage. Per-stage futures are attached via
  `tracing::Instrument` so the spans correctly survive `.await`
  suspension. When a `tracing-subscriber` is set up and the caller is
  already inside a span (e.g. `smtp.conn` on mailrs-server's connection
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

[Unreleased]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.3...HEAD
[1.0.3]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.2...mailrs-inbound-v1.0.3
[1.0.2]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.1...mailrs-inbound-v1.0.2
[1.0.1]: https://github.com/goliajp/mailrs/compare/mailrs-inbound-v1.0.0...mailrs-inbound-v1.0.1
[1.0.0]: https://github.com/goliajp/mailrs/releases/tag/mailrs-inbound-v1.0.0
