//! What delivery records about a message for the sectioned inbox
//! (`mailrs_mailbox::section`): read once, here, for both cores.

use kevy_client::Connection;
use mailrs_core_sidestate::families::contacts;
use mailrs_core_sidestate::families::correspondent::first_address;
use mailrs_mailbox::section::MessageFacts;

/// The facts for `addr`'s copy of a message. `net` is read before the
/// message's own contact record is written, so `first_contact` and
/// `mutual` describe the history before it. `invite` is the caller's to
/// set: it reads the whole body, and this reads only the head. Without a network store nothing is known about
/// the sender, and a message is not called a first contact on no evidence.
pub fn facts(
    net: Option<&mut Connection>,
    addr: &str,
    from: &str,
    to: &str,
    head: &[u8],
    category: &str,
    is_own: bool,
) -> MessageFacts {
    let message = mailrs_rfc5322::Message::new(head);
    let cc = message.header_str("Cc").unwrap_or_default();
    if is_own {
        let to_others = mailrs_rfc5322::split_list(to)
            .chain(mailrs_rfc5322::split_list(cc))
            .map(mailrs_rfc5322::addr_spec)
            .any(|a| !a.trim().is_empty() && !a.trim().eq_ignore_ascii_case(addr));
        return MessageFacts {
            to_others,
            ..MessageFacts::default()
        };
    }
    let sender = first_address(from);
    let headers = String::from_utf8_lossy(head);
    let bucket = mailrs_mailbox::scope::bucket_of(category);
    let machine = mailrs_clean::is_automated_sender(&sender)
        || mailrs_clean::detect_bulk_sender(&headers)
        || matches!(
            bucket,
            mailrs_mailbox::scope::Bucket::Notifications
                | mailrs_mailbox::scope::Bucket::Promotions
        );
    let (first_contact, mutual) = match net {
        Some(conn) if !sender.is_empty() => (
            !contacts::known(conn, addr, &sender),
            mutual(conn, addr, &sender),
        ),
        _ => (false, false),
    };
    MessageFacts {
        direct: mailrs_rfc5322::list_contains(to, addr),
        from_person: !machine,
        first_contact,
        to_others: false,
        invite: false,
        mutual,
    }
}

/// Whether `addr` and `sender` (a `From:` value or a bare address) write
/// to each other, by the counts on the network store now. Delivery reads
/// it before the message's own contact record is written.
pub fn mutual(conn: &mut Connection, addr: &str, sender: &str) -> bool {
    let sender = first_address(sender);
    if sender.is_empty() {
        return false;
    }
    let (sent, received) = contacts::counts(conn, addr, &sender);
    mailrs_mailbox::section::mutual(u64::from(sent), u64::from(received))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "me@x.test";

    fn of(from: &str, to: &str, head: &str, category: &str, own: bool) -> MessageFacts {
        facts(None, ME, from, to, head.as_bytes(), category, own)
    }

    fn counts(c: &mut Connection, who: &str, sent: &[u8], received: &[u8]) {
        c.hset(
            contacts::contact_key(ME, who).as_bytes(),
            &[
                (b"sent_count" as &[u8], sent),
                (b"received_count" as &[u8], received),
            ],
        )
        .unwrap();
    }

    #[test]
    fn a_correspondent_is_one_written_to_a_fifth_as_often() {
        let mut c = Connection::connect("mem://section-facts-mutual").expect("kevy");
        counts(&mut c, "ann@y.test", b"1", b"5");
        counts(&mut c, "desk@y.test", b"3", b"132");
        let ann = facts(
            Some(&mut c),
            ME,
            "Ann <ann@y.test>",
            ME,
            b"",
            "inbox",
            false,
        );
        assert!(ann.mutual);
        let desk = facts(Some(&mut c), ME, "desk@y.test", ME, b"", "inbox", false);
        assert!(!desk.mutual && !desk.first_contact);
        let stranger = facts(Some(&mut c), ME, "eve@z.test", ME, b"", "inbox", false);
        assert!(!stranger.mutual);
        // no store: nothing known, so not a correspondent
        assert!(!of("ann@y.test", ME, "", "inbox", false).mutual);
    }

    #[test]
    fn a_person_writing_to_me_is_direct_and_a_person() {
        let f = of(
            "Ann <ann@y.test>",
            "me@x.test",
            "Subject: hi\r\n",
            "inbox",
            false,
        );
        assert!(f.direct && f.from_person);
        let cc = of(
            "Ann <ann@y.test>",
            "bob@y.test",
            "Cc: me@x.test\r\n",
            "inbox",
            false,
        );
        assert!(!cc.direct && cc.from_person);
    }

    #[test]
    fn machines_and_lists_are_not_people() {
        assert!(!of("noreply@shop.test", ME, "", "inbox", false).from_person);
        let list = "List-Unsubscribe: <mailto:u@list.test>\r\nList-Id: <l.list.test>\r\n";
        assert!(!of("news@list.test", ME, list, "inbox", false).from_person);
        assert!(!of("ann@y.test", ME, "", "promotion", false).from_person);
    }

    #[test]
    fn my_own_mail_records_only_whether_it_went_to_someone() {
        let note = of(ME, ME, "", "inbox", true);
        assert_eq!(note, MessageFacts::default());
        assert!(of(ME, ME, "Cc: ann@y.test\r\n", "inbox", true).to_others);
        assert!(of(ME, "Ann <ann@y.test>", "", "inbox", true).to_others);
    }

    #[test]
    fn a_first_contact_is_a_sender_with_no_history() {
        let mut c = Connection::connect("mem://section-facts-first").expect("kevy");
        let first = facts(Some(&mut c), ME, "ann@y.test", ME, b"", "inbox", false);
        assert!(first.first_contact);
        c.hset(
            contacts::contact_key(ME, "ann@y.test").as_bytes(),
            &[(b"sent_count" as &[u8], b"1" as &[u8])],
        )
        .unwrap();
        let known = facts(Some(&mut c), ME, "ann@y.test", ME, b"", "inbox", false);
        assert!(!known.first_contact);
        contacts::accept(&mut c, ME, "bob@y.test", 5).unwrap();
        let accepted = facts(Some(&mut c), ME, "bob@y.test", ME, b"", "inbox", false);
        assert!(!accepted.first_contact);
        // no store: no evidence either way, so not a first contact
        assert!(!of("carol@y.test", ME, "", "inbox", false).first_contact);
    }
}
