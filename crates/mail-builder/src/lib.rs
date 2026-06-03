#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Outbound mail builder — the inverse of the mailrs parse stones.
//!
//! Constructs canonically-compliant RFC 5322 / 2046 / 2047 / 2231
//! raw bytes for outbound delivery. Lives across from the parse
//! stones ([`mailrs-rfc5322`], [`mailrs-rfc2047`], [`mailrs-mime`])
//! so the same MIME compliance invariants we enforce on inbound
//! can be enforced on outbound — the same canonical envelope, the
//! same encoded-word boundaries, the same CTE-selection heuristics.
//!
//! ## Why a builder stone
//!
//! Outbound message construction in mail servers tends to grow
//! ad-hoc string formatting that drifts toward MIME non-compliance
//! over time (lone LF, mis-folded headers, bad boundaries, missing
//! `Content-Transfer-Encoding`). When a message looks merely
//! "weird" rather than "broken", receiving MTAs silently lower
//! reputation rather than reject — the failure mode is "we got
//! banned without ever seeing a 5xx". A canonical builder closes
//! that whole class of bug at the source.
//!
//! ## Scope
//!
//! - Plain-text single-part messages.
//! - `multipart/alternative` (text + html).
//! - `multipart/mixed` (body + attachments).
//! - Encoded-word (RFC 2047) for non-ASCII header values.
//! - Soft-fold (RFC 5322 §2.2.3) at 78 chars.
//! - CTE auto-selection: 7bit / quoted-printable / base64.
//! - Boundary collision-scan (regenerate if body contains it).
//!
//! Out of scope (deliberate): S/MIME, OpenPGP/MIME, calendar
//! invites (use [`mailrs-ical`]), DKIM signing (use
//! [`mailrs-dkim`]), DSN formatting (use
//! [`mailrs-outbound-queue::dsn`] — to be migrated onto this
//! builder in ckpt 1).
//!
//! ## Status
//!
//! 0.1 — initial MVP. The API is intentionally narrow to match
//! the three internal mailrs use cases (DSN, DMARC report, future
//! TLS-RPT). Wider API surface lands in 1.0 after the deliverability
//! hardening pass (ckpt 2 in the v8 RFC).
//!
//! [`mailrs-rfc5322`]: https://crates.io/crates/mailrs-rfc5322
//! [`mailrs-rfc2047`]: https://crates.io/crates/mailrs-rfc2047
//! [`mailrs-mime`]: https://crates.io/crates/mailrs-mime
//! [`mailrs-ical`]: https://crates.io/crates/mailrs-ical
//! [`mailrs-dkim`]: https://crates.io/crates/mailrs-dkim

mod builder;
mod encode;
mod multipart;
mod strict;

pub use builder::{Attachment, MessageBuilder};
pub use encode::{ContentTransferEncoding, choose_cte};
pub use multipart::{PartBytes, generate_boundary, multipart_envelope};
pub use strict::{LintError, lint};
