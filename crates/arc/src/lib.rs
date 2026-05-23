#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! RFC 8617 Authenticated Received Chain (ARC).
//!
//! ARC extends DKIM / SPF / DMARC across forwarders. Each forwarding hop
//! adds a triplet of headers, indexed by an *instance number* `i=N`:
//!
//! - **ARC-Authentication-Results** (AAR) — the receiver's
//!   `Authentication-Results` snapshot at this hop.
//! - **ARC-Message-Signature** (AMS) — a DKIM-like signature over the
//!   message at this hop's view.
//! - **ARC-Seal** (AS) — a seal that signs the chain so far
//!   (this hop's AAR + AMS + the prior hop's AS).
//!
//! Conformant downstream verifiers walk the chain from `i=1` upward,
//! verify each set's AMS against the message and each set's AS against
//! the chain prefix, and reach a single chain-validation verdict
//! (`pass` / `fail`) that downstream DMARC can override forwarder
//! reputation with.
//!
//! ## What this crate covers (1.0)
//!
//! - [`header`] — parsers for the three header value shapes (AAR, AMS,
//!   AS). They share a tag-list syntax with DKIM-Signature so the
//!   scanner here is byte-for-byte equivalent in shape to
//!   [`mailrs_dkim::DkimHeader`].
//! - [`chain`] — `ArcSet { i, aar, ams, seal }` + `ArcChain::extract`
//!   to pull all sets out of a raw message and group them by instance.
//! - [`verify`] — `verify_chain(&ArcChain, &resolver, raw_message)`
//!   walks the chain in instance order and returns
//!   [`ChainOutcome::Pass`] / `Fail { reason }`.
//!
//! Cryptography (canonicalization + RSA-SHA256 / Ed25519-SHA256
//! signature verify) is delegated to [`mailrs_dkim`] — RFC 8617 §5
//! says ARC-Message-Signature uses the same algorithms and
//! canonicalization as DKIM-Signature, so we route through the
//! battle-tested implementation instead of duplicating ~400 LOC of
//! header / body canon.
//!
//! ## What this crate does NOT cover (1.0)
//!
//! - **ARC sealing** (adding a new ARC set on outbound forward). A
//!   1.1 release will add it; sealing requires DKIM signing key
//!   management, which deserves its own surface area.
//! - **ARC-Reject mode** policy decisions — that's a server-level
//!   concern; this crate returns the verdict, the server enforces.

pub mod chain;
pub mod crypto;
pub mod error;
pub mod header;
pub mod resolver;
pub mod seal;
pub mod verify;

pub use chain::{ArcChain, ArcSet};
pub use crypto::{verify_ams, verify_as};
pub use error::ArcError;
pub use header::{Algorithm, ArcAuthResults, ArcMessageSignature, ArcSeal, ArcSealCv, Canon};
pub use resolver::ArcResolver;
pub use seal::{ArcSigningKey, SealOpts, SealedHeaders, seal};
pub use verify::{ChainOutcome, verify_chain, verify_chain_with_crypto};
