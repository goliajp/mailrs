# Changelog

All notable changes to `mailrs-fraud` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

### Added

- Initial release. Structural signals that a message is a fraud
  attempt, independent of SPF, DKIM and DMARC (which a fraudster with a
  freshly registered domain passes).
- `scan(&Facts, &Policy) -> Findings` runs every check over the facts a
  host extracted from one message (`Facts`, with attachment text capped
  at `ATTACHMENT_TEXT_CAP`). `Policy` holds the organisation's names,
  its own sending domains, domains allowed to carry its name, and the
  display names of its account holders; empty fields turn the checks
  that need them off.
- `Finding` / `Findings`: each finding names its rule, its `Layer`
  (`Transport`, `Identity`, `Provenance`, `Content`), its score and
  whether it is enough to hold the message (`Findings::hold_worthy`).
  `Findings::score` sums the scores.
- Rule ids and scores as constants (`RULE_*`, `*_SCORE`), covering:
  a display name or subject claiming the organisation's or a known
  brand's name from a foreign domain, impersonation of the
  deployment's own account holders, generated `X-Mailer` strings,
  generated sender addresses and sending hostnames, hostnames that
  claim a company, reply-to domain rotation, greetings addressed to
  another person, invisible characters inside words or names,
  bidirectional overrides in display names, and executable
  attachments.
- Modules for each check (`advance_fee`, `attachment`, `brand`,
  `greeting`, `hostname_claim`, `impersonation`, `mailer_fingerprint`,
  `minted_address`, `reply_rotation`, `sending_host`,
  `subject_is_our_name`) for callers that want one signal on its own.
- `testing` feature: `findings_for(rules)` builds `Findings` by rule id
  for downstream tests.
