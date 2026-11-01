//! The webhook events a send's outcome raises: one per recipient, per
//! attempt, for mail the user composed.

use mailrs_core_sidestate::families::webhook_events::{self, kinds};

use super::outcome::{Outcome, extract_code};

/// The event an outcome is, with its SMTP code and the remote's words.
/// A temporary failure is `mail.deferred` while attempts remain and
/// `mail.bounced` on the last one.
pub(super) fn event_of(
    outcome: &Outcome,
    attempts: u32,
    max_attempts: u32,
) -> (&'static str, u16, Option<&str>) {
    match outcome {
        Outcome::Delivered => (kinds::MAIL_DELIVERED, 250, None),
        Outcome::Permanent(r) => (kinds::MAIL_BOUNCED, extract_code(r), Some(r.as_str())),
        Outcome::Transient(r) if attempts < max_attempts => {
            (kinds::MAIL_DEFERRED, extract_code(r), Some(r.as_str()))
        }
        Outcome::Transient(r) => (kinds::MAIL_BOUNCED, extract_code(r), Some(r.as_str())),
    }
}

/// Queue `kind` for `user`'s send `send_id` to `recipient`. Logged, never
/// returned: the send's outcome is recorded whether or not this lands.
pub(super) fn emit(
    kevy_url: &str,
    kind: &'static str,
    user: &str,
    send_id: &str,
    recipient: &str,
    code: u16,
    reason: Option<&str>,
) {
    let Ok(mut conn) = super::kevy(kevy_url) else {
        tracing::warn!(%send_id, kind, "webhook: no kevy connection");
        return;
    };
    let mut data = serde_json::json!({
        "send_id": send_id,
        "recipient": recipient,
        "code": code,
    });
    if let Some(r) = reason {
        data["reason"] = webhook_events::reason(r).into();
    }
    let event = webhook_events::Event {
        kind,
        user,
        sender: None,
        thread_id: None,
        data,
    };
    webhook_events::enqueue_logged(&mut conn, &event, super::now_secs());
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_core_sidestate::families::{webhook_outbox, webhooks};

    #[test]
    fn each_outcome_is_its_event() {
        assert_eq!(event_of(&Outcome::Delivered, 1, 5).0, kinds::MAIL_DELIVERED);
        let bounced = Outcome::Permanent("mx 550 5.1.1 no such user".into());
        assert_eq!(
            event_of(&bounced, 1, 5),
            (kinds::MAIL_BOUNCED, 550, Some("mx 550 5.1.1 no such user"))
        );
        let later = Outcome::Transient("mx 451 try later".into());
        assert_eq!(event_of(&later, 2, 5).0, kinds::MAIL_DEFERRED);
        assert_eq!(
            event_of(&later, 5, 5).0,
            kinds::MAIL_BOUNCED,
            "the last try"
        );
    }

    /// Delivered, deferred and bounced each reach a subscription to them,
    /// with the send, the recipient and the code.
    #[test]
    fn each_delivery_event_is_queued_for_its_subscription() {
        for (n, (kind, code, reason)) in [
            (kinds::MAIL_DELIVERED, 250, None),
            (kinds::MAIL_DEFERRED, 451, Some("mx 451 try later")),
            (kinds::MAIL_BOUNCED, 550, Some("mx 550 no such user")),
        ]
        .into_iter()
        .enumerate()
        {
            let url = format!("mem://sender-webhook-{n}");
            let mut c = kevy_client::Connection::connect(&url).unwrap();
            webhooks::create(
                &mut c,
                webhooks::NewWebhook {
                    account_address: "alice@example.test".into(),
                    url: "https://hooks.example/in".into(),
                    event_type: kind.into(),
                    filter_sender: None,
                    filter_thread_id: None,
                },
            )
            .unwrap();
            emit(
                &url,
                kind,
                "alice@example.test",
                "s1",
                "bob@x.test",
                code,
                reason,
            );
            let got = webhook_outbox::claim_due(&mut c, i64::MAX, 10).unwrap();
            assert_eq!(got.len(), 1, "{kind}");
            let p: serde_json::Value = serde_json::from_str(&got[0].payload).unwrap();
            assert_eq!(p["event"], kind);
            assert_eq!(p["data"]["send_id"], "s1");
            assert_eq!(p["data"]["recipient"], "bob@x.test");
            assert_eq!(p["data"]["code"], code);
            assert_eq!(p["data"].get("reason").and_then(|r| r.as_str()), reason);
        }
    }
}
