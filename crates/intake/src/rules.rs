//! The user's rules, run on mail as it arrives. Each core calls
//! [`matching`] once the message is stored and does what it returns to the
//! message's conversation; both cores ask the same rules the same way.

use mailrs_core_sidestate::families::rules as stored;
use mailrs_mailbox::rules::{Action, Arriving, matches};

use crate::arrival::Arrival;

/// What `addr`'s enabled rules do to this arrival, in rule order without
/// repeats. Mail held as fraud gets nothing; mail in Junk only what cannot
/// take it out of Junk.
pub fn matching(
    net: Option<kevy_client::Connection>,
    addr: &str,
    a: &Arrival<'_>,
    held: bool,
    in_junk: bool,
) -> Vec<Action> {
    if held || a.is_own {
        return Vec::new();
    }
    let Some(mut conn) = net else {
        return Vec::new();
    };
    let rules = match stored::list(&mut conn, addr) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, %addr, "rules: reading the rules failed; none run");
            return Vec::new();
        }
    };
    let arriving = Arriving {
        subject: &a.subject,
        from: &a.from,
        to: &a.to,
        date: a.date,
        attachments: a.parsed.shown_attachment_count() as u32,
        body: a.body_text.as_deref().unwrap_or(""),
    };
    let mut out: Vec<Action> = Vec::new();
    for rule in rules.iter().filter(|r| r.enabled) {
        if !matches(&rule.query, &arriving) {
            continue;
        }
        for action in rule.actions.iter().filter_map(|s| Action::parse(s)) {
            if in_junk && !action.allowed_in_junk() {
                continue;
            }
            if !out.contains(&action) {
                out.push(action);
            }
        }
    }
    out
}
