//! One spool file at a time: decode it, send each recipient's copy where
//! its route says, and remove the file once any copy was taken. A file no
//! recipient took stays in the spool, and is set aside after a week.

use std::collections::HashSet;
use std::path::Path;

use mailrs_core::spool::{SpoolEnvelope, decode_spool_blob};

use crate::domains::relays_outward;
use crate::maildir::{canonical_recipient, ensure_maildir};
use crate::mock;
use crate::outbound::{enqueue_outbound, enqueue_submission};
use crate::route::{Directory, Route, route};
use crate::spool::{aging, stuck};

/// A core the drain delivers into: its directory, the network kevy the
/// side state lives in, and what it does with a copy that reached one of
/// its mailboxes.
pub trait Core: Directory {
    /// A connection to the shared network kevy; `None` when none is
    /// configured or it cannot be reached.
    fn net_conn(&self) -> Option<kevy_client::Connection>;
    /// Deliver to `mailbox`, the mailbox `fwd` resolved to. Returns whether
    /// the message was consumed for this recipient (delivered, filed,
    /// redirected, or deliberately discarded / rejected by sieve).
    fn deliver_resolved(&self, msg: &Inbound<'_, Self>, fwd: &str, mailbox: &str) -> bool;
}

/// The spooled message being delivered.
pub struct Inbound<'a, C: ?Sized> {
    pub core: &'a C,
    pub filename: &'a str,
    pub body: &'a [u8],
    pub env: &'a SpoolEnvelope,
}

/// How many copies of the message were consumed, and the addresses left
/// unresolved.
pub struct Copies {
    pub delivered: usize,
    pub unresolved: Vec<String>,
}

impl Copies {
    pub fn none() -> Self {
        Self {
            delivered: 0,
            unresolved: Vec::new(),
        }
    }

    fn done() -> Self {
        Self {
            delivered: 1,
            unresolved: Vec::new(),
        }
    }

    fn missed(addr: &str) -> Self {
        Self {
            delivered: 0,
            unresolved: vec![addr.to_string()],
        }
    }
}

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// One envelope recipient's copy. `raw` is the recipient as received,
    /// `fwd` its canonical form.
    pub fn deliver_to(&self, raw: &str, fwd: &str) -> Copies {
        let found = match route(self.core, raw, fwd) {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!(%fwd, error = %e, "recipient lookup failed; retried next tick");
                return Copies::missed(fwd);
            }
        };
        let mailbox = match found {
            // never delivered locally: relayed back to whoever we forwarded for
            Route::Srs(original) => {
                return match enqueue_outbound(&original, "", self.body) {
                    Ok(()) => {
                        tracing::info!(srs = %raw, %original, "SRS bounce relayed");
                        Copies::done()
                    }
                    Err(e) => {
                        tracing::warn!(srs = %raw, error = %e, "SRS relay failed");
                        Copies::none()
                    }
                };
            }
            Route::Group(group) => return self.deliver_to_group(fwd, &group),
            Route::Nobody => return Copies::missed(fwd),
            Route::Own => fwd.to_string(),
            Route::Alias(target) => {
                tracing::info!(orig = %fwd, aliased = %target, "spool alias resolved");
                target
            }
        };
        match ensure_maildir(self.core.maildir_root(), &mailbox)
            && self.core.deliver_resolved(self, fwd, &mailbox)
        {
            true => Copies::done(),
            false => Copies::missed(&mailbox),
        }
    }
}

/// Walk one spool dir once, deliver every decodable file to its
/// recipients, and remove it. Returns delivered count and every filename
/// walked — the caller unions the names across the `new` and `cur` dirs
/// before expiring stuck-file reports, because a per-dir expiry would let
/// the empty dir forget the other's files and re-warn them on the very
/// next tick.
pub fn drain_once<C: Core + ?Sized>(dir: &Path, core: &C) -> (usize, HashSet<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(dir = %dir.display(), error = %e, "spool dir read");
            }
            return (0, HashSet::new());
        }
    };
    let mut delivered = 0;
    let mut seen: HashSet<String> = HashSet::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        seen.insert(filename.clone());
        delivered += usize::from(drain_file(dir, &path, &filename, core));
    }
    (delivered, seen)
}

/// One file. True when it was consumed and removed.
fn drain_file<C: Core + ?Sized>(dir: &Path, path: &Path, filename: &str, core: &C) -> bool {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "spool read");
            return false;
        }
    };
    let (env, body) = match decode_spool_blob(&bytes) {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "spool decode; skipping");
            return false;
        }
    };
    if let Some(stored) = mock::drain_sandbox(core, &env, filename, path, body) {
        return stored;
    }
    let inbound = Inbound {
        core,
        filename,
        body,
        env: &env,
    };
    let (delivered_here, unresolved) = inbound.each_recipient();
    if delivered_here == 0 {
        if !aging::set_aside_if_stale(dir, path, filename) && stuck::is_new(filename) {
            // Warn once per file, not once per tick: three undeliverable
            // messages once reprinted the same two lines every 15 s for 11
            // days until the signal was indistinguishable from noise. The
            // file still stays put for a human.
            tracing::warn!(
                file = %filename,
                fwd_paths = ?env.forward_paths,
                "spool file has no resolvable recipient; leaving in place"
            );
        }
        return false;
    }
    if !unresolved.is_empty() {
        tracing::warn!(
            file = %filename,
            unresolved = ?unresolved,
            "spool delivered to some recipients only"
        );
    }
    match std::fs::remove_file(path) {
        Ok(()) => {
            tracing::info!(
                file = %filename,
                from = %env.reverse_path,
                to = ?env.forward_paths,
                "spool → maildir"
            );
            true
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "spool remove after deliver");
            false
        }
    }
}

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// Every envelope recipient's copy: how many were consumed, and who was
    /// left unresolved.
    fn each_recipient(&self) -> (usize, Vec<String>) {
        let env = self.env;
        let mut delivered_here = 0usize;
        let mut unresolved: Vec<String> = Vec::new();
        for (rcpt_index, raw_fwd) in env.forward_paths.iter().enumerate() {
            // Fold once, before anything is looked up. Maildir paths and the
            // account and alias keys are written lowercase, so an uppercase
            // RCPT TO used verbatim misses all three and the message is
            // accepted at SMTP and then never delivered. Everything
            // downstream gets the canonical form too, so the address a human
            // reads in a log is the one the lookups used.
            let fwd = &canonical_recipient(raw_fwd);
            let from = mock::Sender::of(env);
            match mock::divert(self.core, from, fwd, rcpt_index, self.filename, self.body) {
                mock::Diverted::No => {}
                mock::Diverted::Done => {
                    delivered_here += 1;
                    continue;
                }
                mock::Diverted::Retry => {
                    unresolved.push(fwd.clone());
                    continue;
                }
            }
            if relays_outward(env.is_authenticated, fwd, &self.core.local_domains()) {
                match enqueue_submission(&env.reverse_path, raw_fwd, self.body) {
                    Ok(()) => delivered_here += 1,
                    Err(e) => {
                        tracing::warn!(to = %raw_fwd, error = %e, "submission relay enqueue failed");
                        unresolved.push(fwd.clone());
                    }
                }
                continue;
            }
            let copies = self.deliver_to(raw_fwd, fwd);
            delivered_here += copies.delivered;
            unresolved.extend(copies.unresolved);
        }
        (delivered_here, unresolved)
    }
}
