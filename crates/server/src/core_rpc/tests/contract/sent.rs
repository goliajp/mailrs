//! The sender's own copy of what they sent, filed by each core from the
//! same bytes: where it threads, how it reads back, and who joins the
//! contacts.

use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};

/// Ids, counters and file names each store mints for itself.
const MINTED: &[&str] = &[
    "id",
    "mailbox_id",
    "uid",
    "modseq",
    "created_at",
    "blob_ref",
];

const NEW: &[u8] = b"Date: Tue, 14 Nov 2023 22:40:00 +0000\r\n\
From: Alice <alice@example.test>\r\n\
To: Carol <carol@partner.test>\r\n\
Subject: Dinner?\r\n\
Message-ID: <sent1@example.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Are you free on Friday?\r\n";

/// A reply into the seeded two-message thread.
const REPLY: &[u8] = b"Date: Tue, 14 Nov 2023 22:50:00 +0000\r\n\
From: Alice <alice@example.test>\r\n\
To: Friend <friend@partner.test>\r\n\
Subject: Re: Hello\r\n\
Message-ID: <sent2@example.test>\r\n\
In-Reply-To: <m2@partner.test>\r\n\
References: <m2@partner.test>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Good to hear from you.\r\n";

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let rcpts = vec![
        "Carol <carol@partner.test>".to_string(),
        "dave@partner.test".to_string(),
    ];
    let filed = d.both(
        "file_sent new conversation",
        fc.file_sent(ALICE, NEW, &rcpts).await,
        pg.file_sent(ALICE, NEW, &rcpts).await,
        &["blob_ref"],
    );
    if let Some((a, _)) = &filed {
        assert_eq!(a.thread_id, "sent1@example.test");
        assert!(a.blob_ref.starts_with(".Sent/"), "{}", a.blob_ref);
    }
    let reply = d.both(
        "file_sent reply",
        fc.file_sent(ALICE, REPLY, &["friend@partner.test".to_string()])
            .await,
        pg.file_sent(ALICE, REPLY, &["friend@partner.test".to_string()])
            .await,
        &["blob_ref"],
    );
    if let Some((a, _)) = &reply {
        assert_eq!(
            a.thread_id, "t-hello@partner.test",
            "the reply left its thread"
        );
    }

    for tid in ["sent1@example.test", "t-hello@partner.test"] {
        d.both(
            &format!("list_thread_messages {tid} after file_sent"),
            fc.list_thread_messages(ALICE, tid).await,
            pg.list_thread_messages(ALICE, tid).await,
            MINTED,
        );
    }
    d.both(
        "list_sent_messages after file_sent",
        fc.list_sent_messages(ALICE).await,
        pg.list_sent_messages(ALICE).await,
        MINTED,
    );
    let sent = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("Sent".into()),
            limit: 50,
            ..Default::default()
        },
    };
    let rows = d.both(
        "list_conversations [Sent] after file_sent",
        fc.list_conversations(ALICE, &sent).await,
        pg.list_conversations(ALICE, &sent).await,
        MINTED,
    );
    if let Some((rows, _)) = rows {
        let row = rows
            .items
            .iter()
            .find(|r| r.thread_id == "sent1@example.test")
            .expect("the sent copy is in Sent");
        assert_eq!(row.unread_count, 0, "the sent copy is unread");
    }

    let contacts = |url: &str| {
        let mut conn = kevy_client::Connection::connect(url).expect("kevy");
        let mut flat = conn
            .hgetall(
                mailrs_core_sidestate::families::contact_cards::autocomplete_key(ALICE).as_bytes(),
            )
            .expect("contacts");
        flat.sort();
        flat
    };
    let (a, b) = (contacts(FASTCORE_KEVY), contacts(PG_CORE_KEVY));
    d.same("contacts after file_sent", &a, &b, &[]);
    assert!(
        a.iter().any(|v| v.as_slice() == b"dave@partner.test"),
        "the Bcc recipient is not a contact"
    );

    // in base64 just over axum's 2 MiB default, which each core's route
    // kept until a sent copy with a few megabytes attached was turned
    // away; no larger, as a debug pg-core under a full gate's load takes
    // longer than the client's 30 s to file 4 MB
    let mut large = b"Date: Tue, 14 Nov 2023 23:00:00 +0000\r\n\
From: Alice <alice@example.test>\r\n\
To: Carol <carol@partner.test>\r\n\
Subject: Report\r\n\
Message-ID: <sent-large@example.test>\r\n\
\r\n"
        .to_vec();
    large.extend(std::iter::repeat_n(b"QUFBQUFBQUFB\r\n".as_slice(), 120_000).flatten());
    let carol = ["carol@partner.test".to_string()];
    let (a, b) = (
        fc.file_sent(ALICE, &large, &carol).await,
        pg.file_sent(ALICE, &large, &carol).await,
    );
    assert!(
        a.is_ok() && b.is_ok(),
        "a core refused a sent copy over 2 MiB: fastcore {:?}, pg-core {:?}",
        a.as_ref().err(),
        b.as_ref().err()
    );
    d.both("file_sent a message larger than 2 MiB", a, b, &["blob_ref"]);
}
