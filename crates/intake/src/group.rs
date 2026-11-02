//! A recipient that is an email group's address is delivered to the
//! group's members: local members like any recipient, outside members
//! forwarded through the outbound queue with SRS. A member that is itself
//! a group is expanded once, never further, so groups that name each
//! other cannot loop.

use std::collections::HashSet;

use mailrs_core_sidestate::families::email_groups::Group;
use mailrs_core_sidestate::families::shared_inbox::{self, ConvRef};

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
        let team = match group.shared {
            true => self.team_conversation(group.id),
            false => None,
        };
        for target in targets(self.core, group) {
            match target {
                Target::Local { member, mailbox } => {
                    if self.deliver_resolved(&member, &mailbox) {
                        self.forward_locally(&mailbox);
                        if let Some(team) = &team {
                            self.tag_team_copy(team, &mailbox);
                        }
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

/// The team conversation a message a team inbox received belongs to, and
/// what each member's copy is tagged with.
pub(crate) struct TeamCopy {
    conv: ConvRef,
    message_id: String,
    assignee: String,
}

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// The team conversation this message joins, `None` when it has no
    /// Message-ID to thread on or the network kevy cannot be reached: the
    /// members still get their copies, only untied.
    fn team_conversation(&self, gid: i64) -> Option<TeamCopy> {
        let head = &self.body[..self.body.len().min(16 * 1024)];
        let (message_id, in_reply_to, references, ..) = crate::headers::extract_headers(head);
        if message_id.is_empty() {
            return None;
        }
        let mut conn = self.core.net_conn()?;
        let found =
            shared_inbox::conv_for_arrival(&mut conn, gid, &message_id, &in_reply_to, &references)
                .and_then(|conv| Ok((conv, shared_inbox::assign::current(&mut conn, conv)?)));
        match found {
            Ok((conv, assignee)) => Some(TeamCopy {
                conv,
                message_id,
                assignee: assignee.unwrap_or_default(),
            }),
            Err(e) => {
                tracing::warn!(gid, error = %e, "team inbox: conversation not found; copies left untied");
                None
            }
        }
    }

    /// Tie `mailbox`'s copy to the team conversation: the tag and current
    /// assignee on their thread, and their thread on the conversation.
    fn tag_team_copy(&self, team: &TeamCopy, mailbox: &str) {
        let tag = team.conv.tag();
        let Some(tid) = self
            .core
            .tag_shared(mailbox, &team.message_id, &tag, &team.assignee)
        else {
            return;
        };
        let recorded = self
            .core
            .net_conn()
            .map(|mut c| shared_inbox::record_copy(&mut c, team.conv, mailbox, &tid));
        if let Some(Err(e)) = recorded {
            tracing::warn!(%mailbox, %tag, error = %e, "team inbox: copy not recorded");
        }
    }
}
