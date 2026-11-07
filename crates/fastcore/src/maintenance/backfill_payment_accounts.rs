//! Teach each reader's payment account history what already arrived.
//!
//! The history (`core-sidestate::families::payment_accounts`) is written
//! as mail arrives: the receive path records the accounts in a message's
//! text, the attachment reader those in its attachments. Until it is
//! filled from the mail already here, every sender reads as having given
//! no account before, so `account_changed` cannot fire on mail from
//! anyone who wrote before the history shipped.
//!
//! Every message of every reader, through the same readings the live
//! writers use, at each message's own arrival: so a second run writes
//! nothing (`changed` is 0), and each account names the earliest message
//! that gave it. Only account keys are recorded, never numbers. The
//! reader's own mail is not recorded. Bounded, paged, dry by default.

use super::prelude::*;
use super::thread_cursor::{After, next, threads_after, users_from};

/// Messages between pauses.
const PAUSE_EVERY: u64 = 200;

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without recording. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    /// The last thread the previous page finished: its `next_after`.
    after_user: Option<String>,
    after_thread: Option<String>,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
}

fn yes() -> bool {
    true
}
fn default_limit() -> u64 {
    5000
}
fn default_pause_ms() -> u64 {
    20
}

/// `POST /v1/admin/maintenance:backfill-payment-accounts?dry_run=false`
pub(crate) async fn backfill_payment_accounts_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    let Some(conn) = state.net_conn() else {
        tracing::error!("backfill-payment-accounts: no network kevy, nothing to record into");
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match super::hangup::run_blocking(move |h| walk(&state, &q, conn, h)).await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "backfill-payment-accounts failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "backfill-payment-accounts: walk panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// What one walk found and recorded.
#[derive(Default)]
struct Counts {
    walked: u64,
    no_file: u64,
    own: u64,
    with_accounts: u64,
    accounts: u64,
    no_arrival: u64,
    changed: u64,
    accounts_changed: u64,
}

fn walk(
    state: &Arc<FastcoreState>,
    q: &BackfillQuery,
    mut conn: kevy_client::Connection,
    hangup: &super::hangup::Hangup,
) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut c = Counts::default();
    let mut stopped_early = false;
    let after = After::from_query(q.after_user.as_deref(), q.after_thread.as_deref());
    let mut last: Option<After> = None;
    'walk: for user in &users_from(users.clone(), after.as_ref()) {
        for tid in threads_after(state, user, after.as_ref()) {
            if c.walked >= q.limit || hangup.hung_up() {
                stopped_early = true;
                break 'walk;
            }
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                c.walked += 1;
                if c.walked.is_multiple_of(PAUSE_EVERY) {
                    std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
                }
                one(state, &mut conn, q.dry_run, user, &mid, &mut c)?;
            }
            last = Some(After {
                user: user.clone(),
                thread: tid.clone(),
            });
        }
    }
    tracing::info!(
        walked = c.walked,
        with_accounts = c.with_accounts,
        changed = c.changed,
        dry_run = q.dry_run,
        "backfill-payment-accounts"
    );
    Ok(serde_json::json!({
        "done": !stopped_early,
        "next_after": next(&last),
        "dry_run": q.dry_run,
        "readers": users.len(),
        "messages_walked": c.walked,
        // far below `messages_walked` means the maildir is not where this
        // process looks, and every count after it is low
        "no_file": c.no_file,
        "own_mail": c.own,
        "with_accounts": c.with_accounts,
        "accounts": c.accounts,
        // carry accounts but no arrival stamp of ours: not recorded
        "no_arrival": c.no_arrival,
        // messages that added an account or moved one to an earlier
        // message; zero on a dry run, and on a run over mail already
        // recorded
        "changed": c.changed,
        "accounts_changed": c.accounts_changed,
    }))
}

fn one(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    dry_run: bool,
    user: &str,
    mid: &str,
    c: &mut Counts,
) -> std::io::Result<()> {
    let Some(raw) = super::fraud_rescan::reading::raw_for_message(state, user, mid) else {
        c.no_file += 1;
        return Ok(());
    };
    let from = mailrs_inbound::from_header(&raw);
    if mailrs_core_sidestate::families::correspondent::first_address(&from)
        == user.to_ascii_lowercase()
    {
        c.own += 1;
        return Ok(());
    }
    let parsed = mailrs_mime::parse(&raw);
    let texts = state.mailbox.attachment_texts(mid).unwrap_or_default();
    let keys = mailrs_inbound::body::payment_ids(
        &parsed,
        &mailrs_inbound::body::attachment_text_for_rules(&texts),
    )
    .account_keys;
    if keys.is_empty() {
        return Ok(());
    }
    c.with_accounts += 1;
    c.accounts += keys.len() as u64;
    let Some(at) = mailrs_inbound::dates::received_epoch(&raw) else {
        // no arrival to record it at; recording it now would put it after
        // mail that came later
        c.no_arrival += 1;
        return Ok(());
    };
    if dry_run {
        return Ok(());
    }
    let n = mailrs_core_sidestate::families::payment_accounts::record(
        conn, user, &from, &keys, at, mid,
    )?;
    c.changed += u64::from(n > 0);
    c.accounts_changed += n as u64;
    Ok(())
}
