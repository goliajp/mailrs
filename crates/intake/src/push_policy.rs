//! Whether one delivered message should ring the reader's devices.
//!
//! One decision for every channel (APNs, Web Push, UnifiedPush), made
//! where both cores finish a delivery. By default only mail to answer and
//! mail from important contacts rings; the reader can change that per
//! sender, per conversation and overall, and set quiet hours.

use mailrs_core_sidestate::families::notify::{self, Mode, Prefs};
use mailrs_mailbox::section::MessageFacts;

/// What the policy made of one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Push,
    /// The reader's settings leave this one out.
    Policy,
    /// It would push, but it arrived in the quiet hours.
    Quiet,
}

impl Verdict {
    /// The counter this verdict is recorded under.
    pub fn outcome(self) -> &'static str {
        match self {
            Verdict::Push => "sent",
            Verdict::Policy => "suppressed_policy",
            Verdict::Quiet => "suppressed_quiet",
        }
    }
}

/// Everything the decision reads, gathered beforehand.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs<'a> {
    pub prefs: Prefs,
    /// `always` / `never` for this sender, if the reader chose.
    pub sender: Option<&'a str>,
    /// `follow` / `mute` for this conversation, if the reader chose.
    pub thread: Option<&'a str>,
    pub important: bool,
    pub facts: MessageFacts,
    /// The arrival's category: `notification` and `promotion` are feeds.
    pub category: &'a str,
    pub now: i64,
}

/// The decision, in order: the conversation's choice, the sender's, the
/// overall mode; quiet hours only hold back what would otherwise push.
pub fn decide(i: &Inputs<'_>) -> Verdict {
    let wanted = match (i.thread, i.sender) {
        (Some("mute"), _) => false,
        (_, Some("never")) => false,
        (Some("follow"), _) => true,
        (_, Some("always")) => true,
        _ => by_mode(i),
    };
    match (wanted, i.prefs.quiet_at(i.now)) {
        (false, _) => Verdict::Policy,
        (true, true) => Verdict::Quiet,
        (true, false) => Verdict::Push,
    }
}

fn by_mode(i: &Inputs<'_>) -> bool {
    let feed = matches!(
        i.category,
        "notification" | "notifications" | "promotion" | "promotions"
    );
    match i.prefs.mode {
        Mode::Off => false,
        Mode::Important => i.important,
        Mode::All => !feed,
        Mode::Default => i.important || (!feed && to_answer(i.facts)),
    }
}

/// Mail the sectioned inbox files under "to reply": a person writing to the
/// reader directly, someone they have dealt with before.
fn to_answer(f: MessageFacts) -> bool {
    f.from_person && f.direct && !f.first_contact
}

/// The decision, and whether the reader wants the content left off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub verdict: Verdict,
    pub hide_content: bool,
}

/// Read the reader's settings for this message and decide.
pub fn should_notify(
    conn: &mut kevy_client::Connection,
    user: &str,
    sender: &str,
    thread_id: &str,
    facts: MessageFacts,
    category: &str,
    now: i64,
) -> Decision {
    let sender = mailrs_rfc5322::addr_spec(sender)
        .trim()
        .to_ascii_lowercase();
    let prefs = notify::prefs(conn, user).unwrap_or_default();
    let by_sender = notify::get_override(conn, &notify::senders_key(user), &sender)
        .ok()
        .flatten();
    let by_thread = notify::get_override(conn, &notify::threads_key(user), thread_id)
        .ok()
        .flatten();
    let important = mailrs_core_sidestate::families::contacts::is_vip(conn, user, &sender);
    let verdict = decide(&Inputs {
        prefs,
        sender: by_sender.as_deref(),
        thread: by_thread.as_deref(),
        important,
        facts,
        category,
        now,
    });
    Decision {
        verdict,
        hide_content: prefs.hide_content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer() -> MessageFacts {
        MessageFacts {
            direct: true,
            from_person: true,
            first_contact: false,
            to_others: false,
        }
    }

    fn inputs<'a>() -> Inputs<'a> {
        Inputs {
            facts: answer(),
            category: "inbox",
            ..Default::default()
        }
    }

    #[test]
    fn by_default_only_mail_to_answer_and_important_contacts_ring() {
        assert_eq!(decide(&inputs()), Verdict::Push);
        let copied = Inputs {
            facts: MessageFacts {
                direct: false,
                ..answer()
            },
            ..inputs()
        };
        assert_eq!(decide(&copied), Verdict::Policy);
        let first = Inputs {
            facts: MessageFacts {
                first_contact: true,
                ..answer()
            },
            ..inputs()
        };
        assert_eq!(decide(&first), Verdict::Policy);
        let feed = Inputs {
            category: "promotion",
            ..inputs()
        };
        assert_eq!(decide(&feed), Verdict::Policy);
        let robot = Inputs {
            facts: MessageFacts::default(),
            important: true,
            ..inputs()
        };
        assert_eq!(
            decide(&robot),
            Verdict::Push,
            "important contacts always ring"
        );
    }

    #[test]
    fn the_modes() {
        let with = |mode, important, category| Inputs {
            prefs: Prefs {
                mode,
                ..Default::default()
            },
            facts: MessageFacts::default(),
            important,
            category,
            ..Default::default()
        };
        assert_eq!(decide(&with(Mode::All, false, "inbox")), Verdict::Push);
        assert_eq!(
            decide(&with(Mode::All, false, "notification")),
            Verdict::Policy
        );
        assert_eq!(
            decide(&with(Mode::Important, false, "inbox")),
            Verdict::Policy
        );
        assert_eq!(decide(&with(Mode::Important, true, "inbox")), Verdict::Push);
        assert_eq!(decide(&with(Mode::Off, true, "inbox")), Verdict::Policy);
    }

    #[test]
    fn conversation_before_sender_before_mode() {
        let off = Prefs {
            mode: Mode::Off,
            ..Default::default()
        };
        let case = |thread, sender| Inputs {
            prefs: off,
            thread,
            sender,
            ..inputs()
        };
        assert_eq!(decide(&case(Some("follow"), None)), Verdict::Push);
        assert_eq!(decide(&case(None, Some("always"))), Verdict::Push);
        assert_eq!(decide(&case(Some("mute"), Some("always"))), Verdict::Policy);
        assert_eq!(
            decide(&case(Some("follow"), Some("never"))),
            Verdict::Policy
        );
        let default_mode = Inputs {
            sender: Some("never"),
            ..inputs()
        };
        assert_eq!(decide(&default_mode), Verdict::Policy);
    }

    #[test]
    fn quiet_hours_hold_back_only_what_would_ring() {
        let quiet = Prefs {
            quiet_start: Some(0),
            quiet_end: Some(24 * 60 - 1),
            ..Default::default()
        };
        let ring = Inputs {
            prefs: quiet,
            now: 3600,
            ..inputs()
        };
        assert_eq!(decide(&ring), Verdict::Quiet);
        let silent = Inputs {
            category: "promotion",
            ..ring
        };
        assert_eq!(decide(&silent), Verdict::Policy);
    }
}
