//! How important an inbound message is: the heuristic scorer
//! (`mailrs-intelligence`) over the message's shape and the sender
//! relationship kept on the network kevy. Each core stores the verdict on
//! its own thread row.

use kevy_client::Connection;
use mailrs_core_sidestate::families::contacts as ct;
use mailrs_core_sidestate::families::correspondent::first_address;
use mailrs_intelligence::importance::{self, ContactFacts, ImportanceLevel, MessageFacts};

/// Message-shape signals, all from `mailrs-clean` pure helpers.
///
/// `head` is the header region, `raw` the whole message: the HTML
/// signals need the body, and a message with no HTML part contributes
/// no tracking-pixel / template / link evidence at all.
///
/// `sender` must be the **bare** address: `is_automated_sender` matches
/// the local part exactly (`noreply`, `postmaster`, ...), so handing it
/// a `From:` header line silently never matches.
pub fn message_facts(sender: &str, head: &[u8], raw: &[u8]) -> MessageFacts {
    let headers = String::from_utf8_lossy(head);

    let mut facts = MessageFacts {
        is_bulk_sender: mailrs_clean::detect_bulk_sender(&headers),
        is_automated: mailrs_clean::is_automated_sender(sender),
        ..MessageFacts::default()
    };

    let root = mailrs_mime::parse(raw);
    let html = root
        .walk()
        .find(|p| p.content_type.mime_type().as_str() == "text/html")
        .and_then(|p| p.body_text());
    if let Some(html) = html {
        let cleaned = mailrs_clean::clean_email_html(&html);
        facts.has_tracking_pixel = cleaned.has_tracking_pixel;
        facts.is_template_heavy = cleaned.is_template_heavy;
        facts.link_count = cleaned.link_count;
    }
    facts
}

/// Read the sender relationship without mutating it.
pub fn read_relationship(
    conn: &mut Connection,
    user: &str,
    sender: &str,
) -> (Option<ContactFacts>, bool) {
    let s = ct::scoring_for(conn, user, sender);
    let replied = ct::has_sent_to_addr(conn, user, sender);
    (
        Some(ContactFacts {
            is_mutual: s.is_mutual,
            is_vip: s.is_vip,
            is_mailing_list: s.is_mailing_list,
            importance_bias: s.importance_bias,
        }),
        replied,
    )
}

/// Score an inbound message from `senders_csv` to `user`, and record the
/// inbound fact the next score reads. `None` when there is no sender.
///
/// No network kevy degrades to "no relationship known" rather than skipping
/// the score: the message-shape signals alone still separate bulk mail from
/// ordinary mail.
///
/// Call only for inbound messages. The user's own reply must not restate
/// the thread's importance.
pub fn score_inbound(
    net: Option<Connection>,
    user: &str,
    senders_csv: &str,
    head: &[u8],
    raw: &[u8],
) -> Option<(ImportanceLevel, f32)> {
    let sender = first_address(senders_csv);
    if sender.is_empty() {
        return None;
    }
    let facts = message_facts(&sender, head, raw);
    let (contact, is_reply_to_my_email) = match net {
        Some(mut conn) => {
            // the relationship before this message, so a message never
            // counts itself as evidence of one
            let rel = read_relationship(&mut conn, user, &sender);
            if let Err(e) = ct::record_inbound(
                &mut conn,
                user,
                &sender,
                "",
                facts.is_bulk_sender,
                facts.is_automated,
            ) {
                tracing::warn!(error = %e, %user, %sender, "importance: inbound contact fact not recorded");
            }
            rel
        }
        None => (None, false),
    };
    let signals = importance::signals_for_inbound(facts, contact, is_reply_to_my_email);
    Some(importance::calculate_importance(&signals))
}
