//! Telling a reader's devices that conversations they had were taken back
//! into Review: one alert per round, however many of theirs it took.

use super::{Tap, client, spawn_apns, truncated};

/// One conversation a round took back.
pub struct RecalledConversation<'a> {
    pub thread_id: &'a str,
    pub sender: &'a str,
    pub subject: &'a str,
}

/// Tell `addr`'s devices, once, that `taken` were moved to Review, under
/// the reader's push settings: nothing when pushes are off or it is their
/// quiet time, only "moved to Review" when they hide what mail says. One
/// conversation is named by its sender and subject; several by how many
/// they are. Tapping opens the first.
pub fn notify_recalled(
    net: Option<&mut kevy_client::Connection>,
    addr: &str,
    taken: &[RecalledConversation<'_>],
) {
    let Some(first) = taken.first() else {
        return;
    };
    let apns = client();
    let web = crate::web_push::vapid();
    if apns.is_none() && web.is_none() {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let Some(conn) = net else {
        tracing::warn!(%addr, "push: no network kevy, the push settings cannot be read; no push");
        return;
    };
    let prefs = mailrs_core_sidestate::families::notify::prefs(conn, addr).unwrap_or_default();
    if prefs.mode == mailrs_core_sidestate::families::notify::Mode::Off || prefs.quiet_at(now) {
        tracing::debug!(%addr, "push: a recall held back by the reader's settings");
        return;
    }
    let (sender, subject) = match taken.len() {
        1 => (first.sender.to_string(), first.subject.to_string()),
        n => (
            "Moved to Review".to_string(),
            format!("{n} conversations you had were moved to Review"),
        ),
    };
    if let Some(vapid) = web {
        let payload =
            crate::web_push::payload(first.thread_id, &sender, &subject, prefs.hide_content);
        crate::web_push::spawn(vapid, addr, payload, now);
    }
    let Some(apns) = apns else {
        return;
    };
    let body = match (prefs.hide_content, taken.len()) {
        (true, 1) => String::new(),
        _ => truncated(&subject, 300),
    };
    // a held conversation still opens on tap, but archive and mark-read
    // are not what a lock screen should offer for mail that was taken back
    let tap = Tap {
        thread_id: first.thread_id.to_string(),
        category: None,
    };
    spawn_apns(apns, addr, "Moved to Review".to_string(), body, tap, now);
}
