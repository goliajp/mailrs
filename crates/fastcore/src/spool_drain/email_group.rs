//! A recipient that is an email group's address is delivered to the
//! group's members: local members like any recipient, outside members
//! forwarded through the outbound queue with SRS. A member that is itself
//! a group is expanded once, never further, so groups that name each
//! other cannot loop.

use std::collections::HashSet;

use kevy_client::Connection;
use mailrs_core_sidestate::families::email_groups::{Group, find_by_address};

use super::deliver::{canonical_recipient, enqueue_redirect};
use super::recipient::{Copies, Inbound, resolve_local};
use super::relay;

fn lookup(conn: &mut Option<Connection>, addr: &str) -> Option<Group> {
    find_by_address(conn.as_mut()?, addr)
        .inspect_err(|e| tracing::warn!(%addr, error = %e, "email group lookup failed"))
        .ok()
        .flatten()
}

enum Target {
    Local { member: String, mailbox: String },
    Outside(String),
    Unresolved(String),
}

impl Inbound<'_> {
    /// Deliver to the members of `group`, whose address is `fwd`.
    pub(super) fn deliver_to_group(&self, fwd: &str, group: &Group) -> Copies {
        let mut conn = self.state.net_conn();
        let mut out = Copies::none();
        for target in self.targets(&mut conn, group) {
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

    /// Every distinct member, nested groups flattened one level. A mailbox
    /// reached through two members (an address and its alias) appears once.
    fn targets(&self, conn: &mut Option<Connection>, group: &Group) -> Vec<Target> {
        let mut seen: HashSet<String> = HashSet::from([canonical_recipient(&group.address)]);
        let mut mailboxes: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        for member in &group.members {
            let member = canonical_recipient(member.trim());
            if !seen.insert(member.clone()) {
                continue;
            }
            match self.classify(member, &mut mailboxes) {
                Class::Target(t) => out.push(t),
                Class::Duplicate => {}
                Class::Unknown(member) => match lookup(conn, &member) {
                    Some(nested) => {
                        for inner in &nested.members {
                            let inner = canonical_recipient(inner.trim());
                            if !seen.insert(inner.clone()) {
                                continue;
                            }
                            match self.classify(inner, &mut mailboxes) {
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

    fn classify(&self, member: String, mailboxes: &mut HashSet<String>) -> Class {
        if let Some(mailbox) = resolve_local(self.state, self.maildir_root, &member) {
            if !mailboxes.insert(mailbox.clone()) {
                return Class::Duplicate;
            }
            return Class::Target(Target::Local { member, mailbox });
        }
        let outside = member
            .rsplit_once('@')
            .is_some_and(|(_, d)| !relay::local_domains(self.state).iter().any(|l| l == d));
        if outside {
            return Class::Target(Target::Outside(member));
        }
        Class::Unknown(member)
    }
}

enum Class {
    Target(Target),
    /// a mailbox an earlier member already reaches
    Duplicate,
    /// on a local domain with no mailbox: a nested group, or nobody
    Unknown(String),
}
