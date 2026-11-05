//! One spool file at a time: decode it, send each recipient's copy where
//! its route says, and remove the file once any copy was taken. A file no
//! recipient took stays in the spool, and is set aside after a week.
//!
//! Every file is attempted under a record on disk (`spool::attempts`): a
//! file the process died on three times is quarantined, and a recipient
//! whose copy an earlier attempt already took is not given it again.

use std::collections::HashSet;
use std::path::Path;

use mailrs_core::spool::{SpoolEnvelope, decode_spool_blob};

use crate::domains::relays_outward;
use crate::maildir::{canonical_recipient, ensure_maildir};
use crate::mock;
use crate::outbound::{enqueue_outbound, enqueue_submission};
use crate::route::{Directory, Route, route};
use crate::spool::attempts::{self, Attempt, Begin};
use crate::spool::{aging, quarantine, stuck};

/// A core the drain delivers into: its directory, the network kevy the
/// side state lives in, and its store.
pub trait Core: Directory {
    /// A connection to the shared network kevy; `None` when none is
    /// configured or it cannot be reached.
    fn net_conn(&self) -> Option<kevy_client::Connection>;
    /// Index `body`, just written to `addr`'s maildir as `blob_ref` (the
    /// file name, with its subfolder when filed into `folder`). `folder`
    /// is where the receiver's verdict or sieve filed it. False when the
    /// store did not take it; the spool file then stays for another try.
    fn ingest(
        &self,
        addr: &str,
        blob_ref: &str,
        body: &[u8],
        folder: &str,
        fraud_verdict: Option<&str>,
    ) -> bool;
    /// Mark `addr`'s conversation holding `message_id` as their copy of
    /// team conversation `tag` (`"{group}:{conversation}"`), assigned to
    /// `assignee` (empty for nobody). The conversation's thread id; `None`
    /// when the message is not in their mailbox (a sieve script discarded
    /// or redirected it) or the store could not be written.
    fn tag_shared(&self, addr: &str, message_id: &str, tag: &str, assignee: &str)
    -> Option<String>;
    /// Whether `user` may put `from` on mail sent for it, decided as the
    /// web send and SMTP submission decide it. A vacation reply a Sieve
    /// script sends is held to the same rule.
    fn may_send_as(&self, user: &str, from: &str) -> bool;
    /// File `body`, which `user` submitted over SMTP AUTH, as their sent
    /// copy ([`crate::sent::file_submitted`]).
    fn file_submitted(&self, user: &str, body: &[u8], recipients: &[String])
    -> std::io::Result<()>;
}

/// The spooled message being delivered.
pub struct Inbound<'a, C: ?Sized> {
    pub core: &'a C,
    pub filename: &'a str,
    pub body: &'a [u8],
    pub env: &'a SpoolEnvelope,
    /// An earlier attempt on this spool file left a record: its copies may
    /// already be in the maildir, under `cur/` as well as `new/`.
    pub retry: bool,
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
    /// The key that makes outbound job `step` to `target`, made for
    /// recipient `of`, be queued once for this spool file however often the
    /// file is drained.
    pub(crate) fn once(&self, step: &str, of: &str, target: &str) -> String {
        use sha2::Digest as _;
        let mut h = sha2::Sha256::new();
        for part in [self.filename, step, of, target] {
            h.update(part.as_bytes());
            h.update([0u8]);
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A mask took a message: when, for its owner's list, and a pending
    /// mask is enabled. The mail is delivered whether or not this is written.
    fn touch_mask(&self, mask: &str) {
        let Some(mut conn) = self.core.net_conn() else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        if let Err(e) = mailrs_core_sidestate::families::masked_email::touch(&mut conn, mask, now) {
            tracing::warn!(%mask, error = %e, "masked address: last message not recorded");
        }
    }

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
                let once = self.once("srs", raw, &original);
                return match enqueue_outbound(&original, "", self.body, Some(&once)) {
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
            Route::Mask { mask, owner } => {
                self.touch_mask(&mask);
                owner
            }
        };
        match ensure_maildir(self.core.maildir_root(), &mailbox)
            && self.deliver_resolved(fwd, &mailbox)
        {
            true => {
                self.forward_locally(&mailbox);
                Copies::done()
            }
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

/// The spool directory `dir` (`<spool>/incoming/new` or `cur`) is in.
fn spool_root_of(dir: &Path) -> &Path {
    dir.parent().and_then(Path::parent).unwrap_or(dir)
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
    let root = spool_root_of(dir);
    let mut attempt = match attempts::begin(root, filename) {
        Begin::Go(a) => a,
        Begin::Quarantine(record) => {
            let domains = core.local_domains();
            quarantine::set_aside(
                root,
                path,
                filename,
                &env,
                bytes.len() as u64,
                record,
                domains.first().map(String::as_str),
            );
            return false;
        }
    };
    let consumed = deliver_file(dir, path, filename, core, &env, body, &mut attempt);
    attempt.finish(root, filename, path.exists());
    consumed
}

fn deliver_file<C: Core + ?Sized>(
    dir: &Path,
    path: &Path,
    filename: &str,
    core: &C,
    env: &SpoolEnvelope,
    body: &[u8],
    attempt: &mut Attempt,
) -> bool {
    if let Some(stored) = mock::drain_sandbox(core, env, filename, path, body) {
        return stored;
    }
    let inbound = Inbound {
        core,
        filename,
        body,
        env,
        retry: attempt.retry,
    };
    // first, as a web send files it before the message is queued: a local
    // recipient's delivery is not to shape the sender's conversation
    inbound.file_senders_copy(attempt);
    let (delivered_here, unresolved) = inbound.each_recipient(attempt);
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
    /// The copy a web send files for its sender, filed for the account that
    /// signed in to submit this. A spool file drained again (a recipient
    /// left for the next tick) finds it filed and files nothing more. Not
    /// filing it holds back no recipient's copy.
    fn file_senders_copy(&self, attempt: &mut Attempt) {
        let Some(user) = self.env.auth_user.as_deref() else {
            return;
        };
        let step = format!("sent {user}");
        if attempt.is_done(&step) {
            return;
        }
        match self.core.is_account(user) {
            Ok(true) => {}
            // a relay login from the users file has no mailbox
            Ok(false) => return,
            Err(e) => {
                tracing::warn!(%user, error = %e, "submission: sender's copy not filed");
                return;
            }
        }
        attempt.start(&step);
        match self
            .core
            .file_submitted(user, self.body, &self.env.forward_paths)
        {
            Ok(()) => attempt.done(&step),
            Err(e) => tracing::warn!(%user, error = %e, "submission: sender's copy not filed"),
        }
    }

    /// Every envelope recipient's copy: how many were consumed, and who was
    /// left unresolved. A recipient an earlier attempt on this file served
    /// counts as consumed and is not served again.
    fn each_recipient(&self, attempt: &mut Attempt) -> (usize, Vec<String>) {
        let mut delivered_here = 0usize;
        let mut unresolved: Vec<String> = Vec::new();
        for (rcpt_index, raw_fwd) in self.env.forward_paths.iter().enumerate() {
            // Fold once, before anything is looked up. Maildir paths and the
            // account and alias keys are written lowercase, so an uppercase
            // RCPT TO used verbatim misses all three and the message is
            // accepted at SMTP and then never delivered. Everything
            // downstream gets the canonical form too, so the address a human
            // reads in a log is the one the lookups used.
            let fwd = &canonical_recipient(raw_fwd);
            let step = format!("rcpt {fwd}");
            if attempt.is_done(&step) {
                delivered_here += 1;
                continue;
            }
            attempt.start(&step);
            let copies = self.one_recipient(rcpt_index, raw_fwd, fwd);
            if copies.delivered > 0 {
                attempt.done(&step);
            }
            delivered_here += copies.delivered;
            unresolved.extend(copies.unresolved);
        }
        (delivered_here, unresolved)
    }

    fn one_recipient(&self, rcpt_index: usize, raw_fwd: &str, fwd: &str) -> Copies {
        let env = self.env;
        let from = mock::Sender::of(env);
        match mock::divert(self.core, from, fwd, rcpt_index, self.filename, self.body) {
            mock::Diverted::No => {}
            mock::Diverted::Done => return Copies::done(),
            mock::Diverted::Retry => return Copies::missed(fwd),
        }
        if relays_outward(env.is_authenticated, fwd, &self.core.local_domains()) {
            let once = self.once("submission", "", raw_fwd);
            return match enqueue_submission(
                &env.reverse_path,
                env.auth_user.as_deref(),
                raw_fwd,
                self.body,
                Some(&once),
            ) {
                Ok(()) => Copies::done(),
                Err(e) => {
                    tracing::warn!(to = %raw_fwd, error = %e, "submission relay enqueue failed");
                    Copies::missed(fwd)
                }
            };
        }
        self.deliver_to(raw_fwd, fwd)
    }
}
