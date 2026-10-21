//! A recipient that is an email group's address is delivered to the
//! group's members: local members like any recipient, outside members
//! forwarded through the outbound queue with SRS. A member that is itself
//! a group is expanded once, never further, so groups that name each
//! other cannot loop.

use std::collections::HashSet;

use mailrs_core_sidestate::families::email_groups::Group;

use crate::drain::{Copies, Core, Inbound};
use crate::maildir::{canonical_recipient, ensure_maildir};
use crate::outbound::enqueue_redirect;
use crate::route::{Directory, local_mailbox};

pub(crate) enum Target {
    Local { member: String, mailbox: String },
    Outside(String),
    Unresolved(String),
}

enum Class {
    Target(Target),
    /// a mailbox an earlier member already reaches
    Duplicate,
    /// on a local domain with no mailbox: a nested group, or nobody
    Unknown(String),
}

/// The local mailbox `fwd` delivers to, its maildir made if it is an
/// account that has none. A lookup error counts as no mailbox here: a
/// group member that cannot be looked up is left unresolved.
pub(crate) fn resolve_local<D: Directory + ?Sized>(dir: &D, fwd: &str) -> Option<String> {
    let mailbox = local_mailbox(dir, fwd)
        .inspect_err(|e| tracing::warn!(%fwd, error = %e, "recipient lookup failed"))
        .ok()
        .flatten()?;
    ensure_maildir(dir.maildir_root(), &mailbox).then_some(mailbox)
}

fn lookup<D: Directory + ?Sized>(dir: &D, addr: &str) -> Option<Group> {
    dir.group(addr)
        .inspect_err(|e| tracing::warn!(%addr, error = %e, "email group lookup failed"))
        .ok()
        .flatten()
}

/// Every distinct member, nested groups flattened one level. A mailbox
/// reached through two members (an address and its alias) appears once.
pub(crate) fn targets<D: Directory + ?Sized>(dir: &D, group: &Group) -> Vec<Target> {
    let mut seen: HashSet<String> = HashSet::from([canonical_recipient(&group.address)]);
    let mut mailboxes: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for member in &group.members {
        let member = canonical_recipient(member.trim());
        if !seen.insert(member.clone()) {
            continue;
        }
        match classify(dir, member, &mut mailboxes) {
            Class::Target(t) => out.push(t),
            Class::Duplicate => {}
            Class::Unknown(member) => match lookup(dir, &member) {
                Some(nested) => {
                    for inner in &nested.members {
                        let inner = canonical_recipient(inner.trim());
                        if !seen.insert(inner.clone()) {
                            continue;
                        }
                        match classify(dir, inner, &mut mailboxes) {
                            Class::Target(t) => out.push(t),
                            Class::Duplicate => {}
                            Class::Unknown(inner) => out.push(Target::Unresolved(inner)),
                        }
                    }
                }
                None => out.push(Target::Unresolved(member)),
            },
        }
    }
    out
}

fn classify<D: Directory + ?Sized>(
    dir: &D,
    member: String,
    mailboxes: &mut HashSet<String>,
) -> Class {
    if let Some(mailbox) = resolve_local(dir, &member) {
        if !mailboxes.insert(mailbox.clone()) {
            return Class::Duplicate;
        }
        return Class::Target(Target::Local { member, mailbox });
    }
    let outside = member
        .rsplit_once('@')
        .is_some_and(|(_, d)| !dir.local_domains().iter().any(|l| l == d));
    if outside {
        return Class::Target(Target::Outside(member));
    }
    Class::Unknown(member)
}

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// Deliver to the members of `group`, whose address is `fwd`.
    pub(crate) fn deliver_to_group(&self, fwd: &str, group: &Group) -> Copies {
        let mut out = Copies::none();
        for target in targets(self.core, group) {
            match target {
                Target::Local { member, mailbox } => {
                    if self.deliver_resolved(&member, &mailbox) {
                        out.delivered += 1;
                    } else {
                        out.unresolved.push(mailbox);
                    }
                }
                Target::Outside(member) => {
                    match enqueue_redirect(fwd, &member, self.body, &self.env.reverse_path) {
                        Ok(()) => out.delivered += 1,
                        Err(e) => {
                            tracing::warn!(group = %fwd, %member, error = %e,
                                "email group forward enqueue failed");
                            out.unresolved.push(member);
                        }
                    }
                }
                Target::Unresolved(member) => out.unresolved.push(member),
            }
        }
        if out.delivered == 0 {
            out.unresolved.push(fwd.to_string());
        }
        tracing::info!(group = %fwd, delivered = out.delivered, "email group expanded");
        out
    }
}
