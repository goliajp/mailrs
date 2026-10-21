//! iTIP / iMIP invites found in a delivered message: projected onto the
//! message row, announced, and reconciled against the user's calendar.

use crate::event_bus::SmtpEvent;

use super::ProcessDeps;

// MRS-4: detect iTIP / iMIP invite parts and project the parsed
// payload onto messages.invite_payload so the web client / macapp can
// render an invite card without re-parsing.
pub(super) async fn project_invite(
    deps: &ProcessDeps,
    user: &str,
    rcpt_folder: &str,
    uid: u32,
    full_message: &[u8],
) {
    let mb_store = &deps.mailbox_store;
    let Some(extracted) = mailrs_ical::mime_part::extract_invite_part(full_message) else {
        return;
    };
    if let Ok(parsed) = mailrs_ical::parse_invite(&extracted.ics_bytes) {
        if let Ok(payload_json) = serde_json::to_value(&parsed) {
            let method_str = format!("{:?}", parsed.method).to_uppercase();
            match mb_store
                .update_invite_payload(user, rcpt_folder, uid, &payload_json, &method_str)
                .await
            {
                Ok(Some(msg_id)) => {
                    deps.event_bus.emit(SmtpEvent::InviteReceived {
                        user: user.to_string(),
                        message_id: msg_id,
                        method: method_str.clone(),
                        uid: parsed.uid.clone(),
                    });
                    reconcile_invite(deps, user, &parsed, &method_str, &extracted.ics_bytes).await;
                }
                Ok(None) => {
                    tracing::debug!(
                        "invite detected but message row not found for {user}/{rcpt_folder}/{uid}"
                    );
                }
                Err(e) => {
                    tracing::warn!("update_invite_payload failed: {e}");
                }
            }
        }
    } else {
        tracing::debug!(
            "text/calendar part found but ical parse failed for {user}/{rcpt_folder}/{uid}"
        );
    }
}

async fn reconcile_invite(
    deps: &ProcessDeps,
    user: &str,
    parsed: &mailrs_ical::ParsedInvite,
    method_str: &str,
    ics_bytes: &[u8],
) {
    // MRS-7: reconcile against the user's own calendar
    // (only touches events the user has already RSVP'd).
    if let Some(ref pool) = deps.outbound_queue {
        let raw_ics = std::str::from_utf8(ics_bytes).unwrap_or("");
        match crate::calendar::reconcile::reconcile_inbound_invite(pool, user, parsed, raw_ics)
            .await
        {
            Ok(outcome) => {
                tracing::info!(
                    user = %user,
                    uid = %parsed.uid,
                    method = %method_str,
                    outcome = ?outcome,
                    "reconcile",
                );
            }
            Err(e) => {
                tracing::warn!("reconcile failed for {user}/{}: {e}", parsed.uid,);
            }
        }
    }
}
