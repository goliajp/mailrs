use std::sync::Arc;

use tokio::sync::mpsc;

use crate::ai_email::{self, LlmConfig};
use crate::event_bus::{EventBus, SmtpEvent};
use crate::message_util;
use mailrs_mailbox::MailboxStore;

/// spawn the background email analyzer — two independent serial workers
pub fn spawn_analyzer(
    config: LlmConfig,
    mailbox_store: Arc<MailboxStore>,
    event_bus: EventBus,
    maildir_root: String,
) {
    let config = Arc::new(config);

    // worker 1: new emails — serial queue, processes as they arrive
    let (tx, rx) = mpsc::unbounded_channel::<(i64, String, String, String, String)>();
    {
        let cfg = config.clone();
        let store = mailbox_store.clone();
        let mr = maildir_root.clone();
        tokio::spawn(async move { new_email_worker(cfg, store, mr, rx).await });
    }

    // listener: pushes new email jobs into the queue
    {
        let store = mailbox_store.clone();
        let mv = config.model_version();
        tokio::spawn(async move { listen_new_messages(store, event_bus, tx, mv).await });
    }

    // worker 2: backfill — serial, one at a time, runs independently
    {
        let cfg = config;
        let store = mailbox_store;
        tokio::spawn(async move { backfill_worker(cfg, store, maildir_root).await });
    }

    eprintln!("AI email analyzer started");
}

/// worker for new emails — drains queue serially
async fn new_email_worker(
    config: Arc<LlmConfig>,
    store: Arc<MailboxStore>,
    maildir_root: String,
    mut rx: mpsc::UnboundedReceiver<(i64, String, String, String, String)>,
) {
    let model_version = config.model_version();
    while let Some((msg_id, user, maildir_id, sender, subject)) = rx.recv().await {
        analyze_with_retry(
            &config, &store, &maildir_root,
            msg_id, &user, &maildir_id, &sender, &subject, &model_version,
        ).await;
    }
}

/// backfill worker — processes old messages one at a time
async fn backfill_worker(
    config: Arc<LlmConfig>,
    store: Arc<MailboxStore>,
    maildir_root: String,
) {
    let model_version = config.model_version();
    let mut done = 0u64;
    let mut failed = 0u64;
    let mut consecutive_fails = 0u32;

    let total = store.count_unanalyzed_messages(&model_version).await.unwrap_or(0);
    if total == 0 {
        eprintln!("AI backfill: all messages up to date (version {model_version})");
        return;
    }
    eprintln!("AI backfill: {total} messages to analyze (version {model_version})");

    loop {
        let batch = match store.list_unanalyzed_message_ids(1, &model_version).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(event = "backfill_query_error", error = %e);
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                continue;
            }
        };

        let Some((msg_id, user, maildir_id, sender, subject)) = batch.into_iter().next() else {
            eprintln!("AI backfill complete: {done} analyzed, {failed} failed");
            break;
        };

        let success = analyze_with_retry(
            &config, &store, &maildir_root,
            msg_id, &user, &maildir_id, &sender, &subject, &model_version,
        ).await;

        if success {
            done += 1;
            consecutive_fails = 0;
            if done % 20 == 0 {
                eprintln!("AI backfill: {done}/{total} analyzed, {failed} failed");
            }
        } else {
            failed += 1;
            consecutive_fails += 1;
            // exponential backoff: 30s, 60s, 120s, 240s, ..., cap at 3600s (1h)
            let wait = (30u64 << consecutive_fails.saturating_sub(1).min(6)).min(3600);
            eprintln!("AI backfill: {consecutive_fails} consecutive failures, waiting {wait}s");
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        }
    }
}

/// listen for new messages and enqueue analysis jobs
async fn listen_new_messages(
    store: Arc<MailboxStore>,
    event_bus: EventBus,
    tx: mpsc::UnboundedSender<(i64, String, String, String, String)>,
    model_version: String,
) {
    let mut rx = event_bus.subscribe();

    loop {
        match rx.recv().await {
            Ok(SmtpEvent::NewMessage { user, .. }) => {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                if let Ok(batch) = store.list_unanalyzed_message_ids(5, &model_version).await {
                    for row in batch {
                        if row.1 == user {
                            let _ = tx.send(row);
                        }
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
}

/// analyze with up to 3 retries
#[allow(clippy::too_many_arguments)]
async fn analyze_with_retry(
    config: &LlmConfig,
    store: &MailboxStore,
    maildir_root: &str,
    message_id: i64,
    user: &str,
    maildir_id: &str,
    sender_raw: &str,
    subject_raw: &str,
    model_version: &str,
) -> bool {
    const BACKOFF: [u64; 2] = [15, 30];

    for attempt in 0..2u32 {
        if attempt > 0 {
            let delay = BACKOFF[attempt as usize - 1];
            eprintln!("AI retry msg={message_id} attempt={} backoff={delay}s", attempt + 1);
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }

        if do_analyze(config, store, maildir_root, message_id, user, maildir_id, sender_raw, subject_raw, model_version).await {
            return true;
        }
    }

    eprintln!("AI analyzer failed after 2 attempts: msg={message_id}");
    false
}

#[allow(clippy::too_many_arguments)]
async fn do_analyze(
    config: &LlmConfig,
    store: &MailboxStore,
    maildir_root: &str,
    message_id: i64,
    user: &str,
    maildir_id: &str,
    sender_raw: &str,
    subject_raw: &str,
    model_version: &str,
) -> bool {
    let raw = match message_util::read_message_raw(maildir_root, user, maildir_id) {
        Some(r) => r,
        None => {
            tracing::debug!(event = "analyzer_no_raw", message_id, "raw message not found");
            return false;
        }
    };

    let (text_body, html_body, _attachments) = message_util::parse_message(&raw);
    let sender = message_util::decode_header(sender_raw);
    let subject = message_util::decode_header(subject_raw);
    let body_text = text_body.as_deref().or(html_body.as_deref()).unwrap_or("");

    let attachment_text = store.get_attachment_texts(message_id).await.unwrap_or_default();
    let body_for_analysis = if attachment_text.is_empty() {
        body_text.to_string()
    } else {
        format!("{body_text}\n\n[Attachment content]\n{attachment_text}")
    };

    // analysis (LLM complete)
    let analysis = match ai_email::analyze_email(config, &sender, &subject, &body_for_analysis).await {
        Some(a) => a,
        None => return false,
    };

    // embedding (separate model, fast)
    let embedding_text = format!("{subject}\n\n{body_for_analysis}");
    let embedding_result = ai_email::generate_embedding(config, &embedding_text).await;

    let people = serde_json::to_value(&analysis.people).unwrap_or_default();
    let dates = serde_json::to_value(&analysis.dates).unwrap_or_default();
    let amounts = serde_json::to_value(&analysis.amounts).unwrap_or_default();
    let action_items = serde_json::to_value(&analysis.action_items).unwrap_or_default();
    let sender_intent = if analysis.sender_intent.is_empty() { "inform" } else { &analysis.sender_intent };

    if let Err(e) = store
        .upsert_email_analysis(
            message_id, &analysis.category, analysis.risk_score as i16,
            &analysis.risk_reason, &analysis.summary,
            &people, &dates, &amounts, &action_items,
            embedding_result.as_deref(), model_version,
            &analysis.clean_text, analysis.requires_action,
            sender_intent, analysis.action_deadline.as_deref(),
        )
        .await
    {
        eprintln!("AI analyzer DB error msg={message_id}: {e}");
        return false;
    }

    if analysis.requires_action {
        let _ = store.boost_importance_for_action(message_id).await;
    }

    eprintln!(
        "AI analyzed msg={} cat={} risk={} action={} intent={} embed={}",
        message_id, analysis.category, analysis.risk_score,
        analysis.requires_action, sender_intent, embedding_result.is_some(),
    );
    true
}
