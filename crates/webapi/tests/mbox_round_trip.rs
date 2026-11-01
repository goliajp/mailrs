//! An mbox imported, exported, imported into another account and exported
//! again comes out the same: every message, its bytes, its folder and
//! whether it was read. Through the front door, against a real fastcore.

mod live;

use mailrs_webapi::handlers::mbox::format::{Folder, Parsed, Splitter};

const KEVY: &str = "mem://webapi-mbox-round-trip";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

/// A Gmail Takeout in miniature: unread and read in the Inbox, archived,
/// sent, and spam that is left behind. One body line starts with `From `,
/// quoted as an mbox writes it.
const TAKEOUT: &str = "From 1@xxx Mon Jan 02 09:00:00 +0000 2023\n\
X-Gmail-Labels: Inbox,Unread\n\
Message-ID: <a@old.test>\n\
From: Erin <erin@old.test>\n\
To: alice@example.test\n\
Subject: Unread one\n\
Date: Mon, 02 Jan 2023 09:00:00 +0000\n\
\n\
>From the start, this line was quoted.\n\
\n\
From 2@xxx Tue Jan 03 09:00:00 +0000 2023\n\
X-Gmail-Labels: Inbox,Important\n\
Message-ID: <b@old.test>\n\
From: Frank <frank@old.test>\n\
To: alice@example.test\n\
Subject: Read one\n\
Date: Tue, 03 Jan 2023 09:00:00 +0000\n\
\n\
Seen already.\n\
\n\
From 3@xxx Wed Jan 04 09:00:00 +0000 2023\n\
X-Gmail-Labels: Opened,\"Category Updates\"\n\
Message-ID: <c@old.test>\n\
From: Shop <orders@shop.test>\n\
To: alice@example.test\n\
Subject: Receipt\n\
Date: Wed, 04 Jan 2023 09:00:00 +0000\n\
\n\
Archived long ago.\n\
\n\
From 4@xxx Thu Jan 05 09:00:00 +0000 2023\n\
X-Gmail-Labels: Sent\n\
Message-ID: <d@old.test>\n\
From: Alice <alice@example.test>\n\
To: Carol <carol@partner.test>\n\
Subject: Lunch?\n\
Date: Thu, 05 Jan 2023 09:00:00 +0000\n\
\n\
Friday?\n\
\n\
From 5@xxx Fri Jan 06 09:00:00 +0000 2023\n\
X-Gmail-Labels: Spam,Unread\n\
Message-ID: <e@old.test>\n\
From: Prize <win@spam.test>\n\
To: alice@example.test\n\
Subject: You won\n\
\n\
Claim now.\n";

async fn import(live: &live::Live, bearer: &str, mbox: Vec<u8>) -> serde_json::Value {
    let res = live
        .http
        .post(format!("{}/api/mail/import", live.base))
        .bearer_auth(bearer)
        .body(mbox)
        .send()
        .await
        .expect("import");
    assert_eq!(res.status(), 200);
    res.json().await.expect("report")
}

async fn export(live: &live::Live, bearer: &str) -> Vec<u8> {
    live.confirm(bearer, "pw").await;
    let res = live
        .http
        .get(format!("{}/api/mail/export", live.base))
        .bearer_auth(bearer)
        .send()
        .await
        .expect("export");
    assert_eq!(res.status(), 200);
    res.bytes().await.expect("mbox").to_vec()
}

/// The messages of an export, in a fixed order.
fn messages(mbox: &[u8]) -> Vec<(Vec<u8>, Folder, bool)> {
    let mut s = Splitter::default();
    let mut out: Vec<Parsed> = s.push(mbox);
    out.extend(s.finish());
    let mut v: Vec<_> = out.into_iter().map(|p| (p.raw, p.folder, p.seen)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_import_exports_and_imports_back_unchanged() {
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    let first = import(&live, &alice, TAKEOUT.as_bytes().to_vec()).await;
    assert_eq!(
        first,
        serde_json::json!({
            "imported": 3, "sent": 1, "archived": 1, "skipped_duplicate": 0,
            "skipped_junk_or_trash": 1, "too_large": 0, "failed": 0, "complete": true,
        })
    );
    let again = import(&live, &alice, TAKEOUT.as_bytes().to_vec()).await;
    assert_eq!(again["skipped_duplicate"], 4, "{again}");
    assert_eq!(again["imported"], 0, "{again}");

    let from_alice = export(&live, &alice).await;
    let alice_mail = messages(&from_alice);
    let shape: Vec<(Folder, bool)> = alice_mail.iter().map(|m| (m.1, m.2)).collect();
    let mut want = vec![
        (Folder::Inbox, false),
        (Folder::Inbox, true),
        (Folder::Archive, true),
        (Folder::Sent, true),
    ];
    let mut got = shape.clone();
    want.sort_by_key(|f| format!("{f:?}"));
    got.sort_by_key(|f| format!("{f:?}"));
    assert_eq!(got, want, "{}", String::from_utf8_lossy(&from_alice));
    assert!(
        String::from_utf8_lossy(&from_alice).contains("\n>From the start"),
        "a body line starting with From is quoted"
    );

    let into_bob = import(&live, &bob, from_alice.clone()).await;
    assert_eq!(into_bob["failed"], 0, "{into_bob}");
    let from_bob = export(&live, &bob).await;
    assert_eq!(
        messages(&from_bob),
        alice_mail,
        "the round trip changed something"
    );
}
