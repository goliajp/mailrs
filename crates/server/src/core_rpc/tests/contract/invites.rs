//! An invitation that arrives through the spool is read back alike from
//! both cores: each stores the typed invitation at ingest.

use super::ALICE;
use super::harness::{Cores, Diffs};
use super::intake::{Spools, envelope};

const RAW: &str = "From: chair@outside.example\r\n\
To: alice@example.test\r\n\
Subject: Offsite\r\n\
Message-ID: <offsite@outside.example>\r\n\
Date: Sun, 1 Nov 2026 09:00:00 +0900\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
\r\n\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:offsite-2026\r\n\
SUMMARY:Offsite\r\n\
DTSTAMP:20261101T000000Z\r\n\
DTSTART:20261103T010000Z\r\n\
DTEND:20261103T020000Z\r\n\
ORGANIZER:mailto:chair@outside.example\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    Spools::new(c)
        .arrive(c, &envelope(None), RAW.as_bytes())
        .await;
    let found = d.both(
        "find the invitation by Message-ID",
        c.fastcore
            .find_by_message_id_for_user(ALICE, "offsite@outside.example")
            .await,
        c.pg_core
            .find_by_message_id_for_user(ALICE, "offsite@outside.example")
            .await,
        &[
            "id",
            "mailbox_id",
            "uid",
            "modseq",
            "blob_ref",
            "internal_date",
        ],
    );
    let Some((fc, pg)) = found else {
        return;
    };
    let invites = d.both(
        "get_invite",
        c.fastcore.get_invite(ALICE, fc.uid).await,
        c.pg_core.get_invite(ALICE, pg.uid).await,
        &[],
    );
    if invites.is_none() {
        d.0.push("get_invite: neither core answered an invitation that arrived".into());
    }
}
