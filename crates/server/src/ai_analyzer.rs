use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::ai_email::{self, GeminiConfig};
use crate::event_bus::{EventBus, SmtpEvent};
use crate::message_util;
use mailrs_mailbox::MailboxStore;

/// spawn the background email analyzer
/// runs backfill on startup, then listens for new messages
pub fn spawn_analyzer(
    config: GeminiConfig,
    mailbox_store: Arc<MailboxStore>,
    event_bus: EventBus,
    maildir_root: String,
) {
    let config = Arc::new(config);

    // backfill existing unanalyzed messages
    let cfg = config.clone();
    let mb = mailbox_store.clone();
    let mr = maildir_root.clone();
    tokio::spawn(async move {
        backfill_loop(cfg, mb, mr).await;
    });

    // listen for new messages
    let cfg = config;
    let mb = mailbox_store;
    tokio::spawn(async move {
        listen_new_messages(cfg, mb, event_bus, maildir_root).await;
    });

    eprintln!("AI email analyzer started");
}

async fn backfill_loop(
    config: Arc<GeminiConfig>,
    store: Arc<MailboxStore>,
    maildir_root: String,
) {
    let semaphore = Arc::new(Semaphore::new(2));
    let model_version = config.model_version();
    let analyzed_count = Arc::new(AtomicU64::new(0));
    let failed_count = Arc::new(AtomicU64::new(0));

    // get total count for progress logging
    let total = store.count_unanalyzed_messages(&model_version).await.unwrap_or(0);
    if total == 0 {
        eprintln!("AI backfill: all messages up to date (version {})", model_version);
        return;
    }
    eprintln!("AI backfill: {total} messages to analyze (version {model_version})");

    loop {
        let batch = match store.list_unanalyzed_message_ids(10, &model_version).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(event = "backfill_query_error", error = %e);
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                continue;
            }
        };

        if batch.is_empty() {
            let done = analyzed_count.load(Ordering::Relaxed);
            let fails = failed_count.load(Ordering::Relaxed);
            eprintln!("AI backfill complete: {done} analyzed, {fails} failed");
            break;
        }

        let mut handles = Vec::new();
        for (msg_id, user, maildir_id, sender, subject) in batch {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let cfg = config.clone();
            let store = store.clone();
            let mr = maildir_root.clone();
            let mv = model_version.clone();
            let counter = analyzed_count.clone();
            let fail_counter = failed_count.clone();

            handles.push(tokio::spawn(async move {
                let success = analyze_with_retry(&cfg, &store, &mr, msg_id, &user, &maildir_id, &sender, &subject, &mv).await;
                if success {
                    let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if done % 20 == 0 {
                        eprintln!("AI backfill: {done}/{total} analyzed");
                    }
                } else {
                    fail_counter.fetch_add(1, Ordering::Relaxed);
                }
                drop(permit);
            }));

            // rate limiting: 1 request per second per slot
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // wait for all in batch to complete
        for h in handles {
            let _ = h.await;
        }

        // pause between batches
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn listen_new_messages(
    config: Arc<GeminiConfig>,
    store: Arc<MailboxStore>,
    event_bus: EventBus,
    maildir_root: String,
) {
    let mut rx = event_bus.subscribe();
    let model_version = config.model_version();

    loop {
        match rx.recv().await {
            Ok(SmtpEvent::NewMessage { user, .. }) => {
                // small delay to let the message be fully stored
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                // find the newest unanalyzed messages for this user
                if let Ok(batch) = store.list_unanalyzed_message_ids(5, &model_version).await {
                    for (msg_id, msg_user, maildir_id, sender, subject) in batch {
                        if msg_user == user {
                            let cfg = config.clone();
                            let store = store.clone();
                            let mr = maildir_root.clone();
                            let mv = model_version.clone();
                            tokio::spawn(async move {
                                analyze_with_retry(&cfg, &store, &mr, msg_id, &msg_user, &maildir_id, &sender, &subject, &mv).await;
                            });
                        }
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
}

/// analyze a single message with up to 5 retries and longer exponential backoff
async fn analyze_with_retry(
    config: &GeminiConfig,
    store: &MailboxStore,
    maildir_root: &str,
    message_id: i64,
    user: &str,
    maildir_id: &str,
    sender_raw: &str,
    subject_raw: &str,
    model_version: &str,
) -> bool {
    const BACKOFF_SECS: [u64; 5] = [10, 30, 60, 120, 300];

    for attempt in 0..5u32 {
        if attempt > 0 {
            let delay = std::time::Duration::from_secs(BACKOFF_SECS[attempt as usize - 1]);
            eprintln!("AI retry msg={message_id} attempt={} backoff={}s", attempt + 1, delay.as_secs());
            tokio::time::sleep(delay).await;
        }

        if analyze_single_message(config, store, maildir_root, message_id, user, maildir_id, sender_raw, subject_raw, model_version).await {
            return true;
        }
    }

    eprintln!("AI analyzer failed after 5 attempts: msg={message_id}");
    false
}

/// returns true on success
async fn analyze_single_message(
    config: &GeminiConfig,
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

    let body_for_analysis = text_body
        .as_deref()
        .or(html_body.as_deref())
        .unwrap_or("");

    // prepare text for embedding: combine subject + body
    let embedding_text = format!("{subject}\n\n{body_for_analysis}");

    // run embedding and analysis concurrently
    let (embedding_result, analysis_result) = tokio::join!(
        ai_email::generate_embedding(config, &embedding_text),
        ai_email::analyze_email(config, &sender, &subject, body_for_analysis),
    );

    let analysis = match analysis_result {
        Some(a) => a,
        None => {
            tracing::debug!(event = "analyzer_no_result", message_id);
            return false;
        }
    };

    let people = serde_json::to_value(&analysis.people).unwrap_or_default();
    let dates = serde_json::to_value(&analysis.dates).unwrap_or_default();
    let amounts = serde_json::to_value(&analysis.amounts).unwrap_or_default();
    let action_items = serde_json::to_value(&analysis.action_items).unwrap_or_default();

    if let Err(e) = store
        .upsert_email_analysis(
            message_id,
            &analysis.category,
            analysis.risk_score as i16,
            &analysis.risk_reason,
            &analysis.summary,
            &people,
            &dates,
            &amounts,
            &action_items,
            embedding_result.as_deref(),
            model_version,
            &analysis.clean_text,
        )
        .await
    {
        eprintln!("AI analyzer DB error msg={message_id}: {e}");
        return false;
    }

    eprintln!(
        "AI analyzed msg={} cat={} risk={} embed={}",
        message_id, analysis.category, analysis.risk_score, embedding_result.is_some()
    );

    true
}
