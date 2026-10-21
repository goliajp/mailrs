//! Delivery and the message-level reads: mailboxes, UIDs, Message-IDs,
//! flags, analysis, invitations.

use mailrs_core_api::method::admin::SetMessageFlagsRequest;
use mailrs_core_api::method::thread::DeliverMessageRequest;

use super::harness::{Cores, Diffs};
use super::{ALICE, BOB};

/// Ids each store mints for itself, the per-store mailbox handle, and the
/// IMAP counters each store keeps in its own sequence.
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "created_at",
    "modseq",
    "uidvalidity",
    "uidnext",
    "highest_modseq",
];

/// One coherent arrival: the request carries the time, the subject and
/// the read state twice (top level and inside the wire payload), and the
/// two cores read different copies, so both copies always agree.
pub(super) struct Arrival<'a> {
    pub message_id: &'a str,
    pub uid: u32,
    pub thread_id: &'a str,
    pub user: &'a str,
    pub sender: &'a str,
    pub subject: &'a str,
    pub date: i64,
    pub category: &'a str,
    pub unread: bool,
}

impl Arrival<'_> {
    fn request(&self) -> DeliverMessageRequest {
        let flags = if self.unread {
            0
        } else {
            mailrs_core_api::method::message::FLAG_SEEN
        };
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": self.uid,
            "blob_ref": format!("{}.host", self.uid),
            "sender": self.sender, "recipients": self.user, "subject": self.subject,
            "date": self.date, "internal_date": self.date,
            "size": 120, "flags": flags, "message_id": self.message_id,
            "in_reply_to": "", "thread_id": self.thread_id, "modseq": 0,
            "user_address": self.user,
        });
        DeliverMessageRequest {
            message_id: self.message_id.into(),
            subject: self.subject.into(),
            senders_csv: self.sender.into(),
            latest_date: self.date,
            latest_preview: format!("about {}", self.subject),
            category: self.category.into(),
            unread: self.unread,
            uid: self.uid,
            payload_wire_json: wire.to_string(),
        }
    }
}

const FRIEND: &str = "Friend <friend@partner.test>";
const SHOP: &str = "Shop <news@shop.test>";
const BILLING: &str = "Billing <noreply@billing.test>";

/// The mailbox every later scenario reads: inbox mail in a two-message
/// thread, a notification, a promotion, junk, mail Alice sent, and one
/// thread Bob shares with her.
pub(super) fn arrivals() -> Vec<Arrival<'static>> {
    vec![
        Arrival {
            message_id: "m1@partner.test",
            uid: 1,
            thread_id: "t-hello@partner.test",
            user: ALICE,
            sender: FRIEND,
            subject: "Hello",
            date: 1_700_000_100,
            category: "inbox",
            unread: false,
        },
        Arrival {
            message_id: "m2@partner.test",
            uid: 2,
            thread_id: "t-hello@partner.test",
            user: ALICE,
            sender: FRIEND,
            subject: "Re: Hello",
            date: 1_700_000_200,
            category: "inbox",
            unread: true,
        },
        Arrival {
            message_id: "m3@billing.test",
            uid: 3,
            thread_id: "t-invoice@billing.test",
            user: ALICE,
            sender: BILLING,
            subject: "Your invoice",
            date: 1_700_000_300,
            category: "notification",
            unread: true,
        },
        Arrival {
            message_id: "m4@shop.test",
            uid: 4,
            thread_id: "t-sale@shop.test",
            user: ALICE,
            sender: SHOP,
            subject: "Sale",
            date: 1_700_000_400,
            category: "promotion",
            unread: true,
        },
        Arrival {
            message_id: "m5@spam.test",
            uid: 5,
            thread_id: "t-spam@spam.test",
            user: ALICE,
            sender: "x@spam.test",
            subject: "You won",
            date: 1_700_000_500,
            category: "spam",
            unread: true,
        },
        Arrival {
            message_id: "m6@example.test",
            uid: 6,
            thread_id: "t-sent@example.test",
            user: ALICE,
            sender: ALICE,
            subject: "Lunch?",
            date: 1_700_000_600,
            category: "inbox",
            unread: false,
        },
        Arrival {
            message_id: "m7@example.test",
            uid: 1,
            thread_id: "t-shared@example.test",
            user: BOB,
            sender: FRIEND,
            subject: "Both of you",
            date: 1_700_000_700,
            category: "inbox",
            unread: true,
        },
        Arrival {
            message_id: "m7@example.test",
            uid: 7,
            thread_id: "t-shared@example.test",
            user: ALICE,
            sender: FRIEND,
            subject: "Both of you",
            date: 1_700_000_700,
            category: "inbox",
            unread: true,
        },
    ]
}

pub(super) async fn seed(c: &Cores, d: &mut Diffs) {
    for a in arrivals() {
        let req = a.request();
        d.both(
            &format!("deliver_message {} for {}", a.message_id, a.user),
            c.fastcore.deliver_message(a.user, a.thread_id, &req).await,
            c.pg_core.deliver_message(a.user, a.thread_id, &req).await,
            MINTED,
        );
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);

    d.both(
        "list_mailboxes",
        fc.list_mailboxes(ALICE).await,
        pg.list_mailboxes(ALICE).await,
        MINTED,
    );

    d.both(
        "get_message_by_uid_for_user",
        fc.get_message_by_uid_for_user(ALICE, 2).await,
        pg.get_message_by_uid_for_user(ALICE, 2).await,
        MINTED,
    );
    d.both(
        "find_by_message_id_for_user",
        fc.find_by_message_id_for_user(ALICE, "m3@billing.test")
            .await,
        pg.find_by_message_id_for_user(ALICE, "m3@billing.test")
            .await,
        MINTED,
    );
    // nothing here is an invitation; both cores must say so the same way
    d.both(
        "get_invite without one",
        fc.get_invite(ALICE, 2).await.map_err(|e| e.to_string()),
        pg.get_invite(ALICE, 2).await.map_err(|e| e.to_string()),
        &[],
    );
    let req = SetMessageFlagsRequest {
        flags: mailrs_core_api::method::message::FLAG_SEEN,
    };
    d.both(
        "set_message_flags",
        fc.set_message_flags(ALICE, 4, &req).await,
        pg.set_message_flags(ALICE, 4, &req).await,
        &[],
    );
    d.both(
        "get_message_by_uid_for_user after flags",
        fc.get_message_by_uid_for_user(ALICE, 4).await,
        pg.get_message_by_uid_for_user(ALICE, 4).await,
        MINTED,
    );
}
