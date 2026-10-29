//! Reports that arrive as mail — DMARC aggregates, feedback-loop
//! complaints, TLS-RPT — read off a delivered copy into the network kevy.
//! The message still lands in the mailbox as ordinary mail.

pub mod dmarc;
pub mod fbl;
mod fbl_evidence;
mod reported_to;
pub mod tlsrpt;

pub use reported_to::reported_to;

/// Every report hook, for the copy addressed to `fwd` and delivered to
/// `mailbox`.
pub fn ingest(maildir_root: &str, fwd: &str, mailbox: &str, body: &[u8]) {
    let addressed = reported_to(fwd, mailbox);
    dmarc::maybe_ingest(addressed, body);
    fbl::maybe_record_complaint(maildir_root, addressed, body);
    tlsrpt::maybe_ingest(addressed, body);
}
