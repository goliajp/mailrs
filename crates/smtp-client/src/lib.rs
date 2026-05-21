#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Outbound SMTP client primitives: MX resolution, DANE/STARTTLS, response parsing.
//!
//! `mailrs-smtp-client` is the send-side counterpart to [`mailrs-smtp-proto`].
//! It is not a full mail user agent — it is the wire-level pieces an MTA needs
//! to deliver a message that has already been built: looking up MX records,
//! choosing the right relay, opening a TLS connection that can be verified
//! against DNSSEC-anchored TLSA records ([RFC 7672] DANE), and reading SMTP
//! replies that wrap across multiple lines.
//!
//! Built on `tokio` + `rustls` + [`hickory_resolver`]. Extracted from
//! [mailrs], a Rust mail server, and published independently so anyone
//! writing an MTA, a delivery-test harness, or a bounce probe in Rust can
//! share the same battle-tested foundation.
//!
//! # Quick start
//!
//! ```no_run
//! use mailrs_smtp_client::{MxCache, SmtpConnection, TokioResolver, sort_mx_records};
//! use std::time::Duration;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let resolver = TokioResolver::builder_tokio()?.build()?;
//! let cache = MxCache::new(Duration::from_secs(300));
//!
//! let mut records = cache.resolve(&resolver, "example.com").await?;
//! sort_mx_records(&mut records);
//! let primary = records.first().ok_or("no MX")?;
//!
//! // connect (reads the banner internally), EHLO, upgrade to TLS, then send
//! let mut conn = SmtpConnection::connect(&primary.exchange, 25).await?;
//! conn.ehlo("client.example.org").await?;
//! let mut conn = conn.starttls(&primary.exchange).await?;
//! conn.ehlo("client.example.org").await?;
//! conn.deliver(
//!     "sender@example.org",
//!     &["bob@example.com"],
//!     b"Subject: hi\r\n\r\nhello\r\n",
//! ).await?;
//! conn.quit().await?;
//! # Ok(()) }
//! ```
//!
//! # Module overview
//!
//! - [`mx`] — DNS MX lookup with preference-sort, in-memory cache, and
//!   `A`-record fallback for domains without an MX.
//! - [`dane`] — TLSA record resolution and certificate verification
//!   ([RFC 7672]) to defend STARTTLS against active MITM.
//! - [`connection`] — [`SmtpConnection`] wraps the TLS-upgradable read/write
//!   loop with per-command timeouts ([`TimeoutConfig`]).
//! - [`response`] — single- and multi-line reply parser
//!   ([RFC 5321 §4.2.1]).
//!
//! # What this crate does NOT do
//!
//! - No DKIM signing, no SPF, no DMARC. Use [`mail-auth`] upstream.
//! - No queue / retry / DSN. See `mailrs-outbound-queue`.
//! - No SMTP server. See [`mailrs-smtp-proto`] for the receive-side state machine.
//!
//! [RFC 5321 §4.2.1]: https://datatracker.ietf.org/doc/html/rfc5321#section-4.2.1
//! [RFC 7672]: https://datatracker.ietf.org/doc/html/rfc7672
//! [`mail-auth`]: https://crates.io/crates/mail-auth
//! [`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
//! [mailrs]: https://github.com/goliajp/mailrs

/// `SmtpConnection` — async TLS / STARTTLS connection state machine + timeouts.
pub mod connection;
/// RFC 7672 DANE (DNS-Based Authentication of Named Entities): TLSA record
/// lookup + TLS verification config.
pub mod dane;
/// MX record lookup + priority sorting + `fallback_to_domain` for
/// MX-less destinations.
pub mod mx;
/// RFC 5321 multi-line response parser (`250-XXX` continuation lines).
pub mod response;

pub use connection::{SmtpConnection, TimeoutConfig};
pub use dane::{DaneVerifier, TlsaRecord, dane_tls_config, resolve_tlsa};
pub use mx::{MxCache, MxRecord, TokioResolver, fallback_to_domain, resolve_mx, sort_mx_records};
pub use response::{SmtpResponse, parse_response};
