//! The phone numbers and payment accounts read out of attachments, into the
//! recipient's histories in the network kevy.

use std::sync::Arc;

use super::AttachmentTextStore;

/// The phone numbers and payment accounts (their keys, never the numbers)
/// in the attachments' text, into `user`'s histories with the sender at
/// the message's arrival, beside the ones delivery recorded from its text.
pub(super) async fn record<S: AttachmentTextStore>(
    store: &Arc<S>,
    user: &str,
    mid: &str,
    raw: Vec<u8>,
    texts: Vec<String>,
) {
    let text = mailrs_inbound::body::attachment_text_for_rules(&texts);
    let numbers: Vec<String> = mailrs_fraud::callback::phone_numbers(&text)
        .into_iter()
        .take(mailrs_fraud::callback::MAX_NUMBERS)
        .collect();
    let accounts = mailrs_fraud::payment::find(&text).account_keys;
    if numbers.is_empty() && accounts.is_empty() {
        return;
    }
    let (store, user, mid) = (store.clone(), user.to_string(), mid.to_string());
    let write = move || {
        let from = mailrs_inbound::from_header(&raw);
        // the reader's own mail, as delivery and the backfill leave it out
        if mailrs_core_sidestate::families::correspondent::first_address(&from)
            == user.to_ascii_lowercase()
        {
            return None;
        }
        let mut conn = store.net_conn()?;
        let at = mailrs_inbound::dates::received_epoch(&raw)
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        let numbers = mailrs_core_sidestate::families::callback_numbers::record_numbers(
            &mut conn, &user, &from, &numbers, at, &mid,
        );
        let accounts = mailrs_core_sidestate::families::payment_accounts::record(
            &mut conn, &user, &from, &accounts, at, &mid,
        );
        Some((numbers, accounts))
    };
    match tokio::task::spawn_blocking(write).await {
        Ok(Some((numbers, accounts))) => {
            if let Err(e) = numbers {
                tracing::warn!(error = %e, "attachment text: callback numbers not recorded");
            }
            if let Err(e) = accounts {
                tracing::warn!(error = %e, "attachment text: payment accounts not recorded");
            }
        }
        Ok(None) => {}
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    }
}
