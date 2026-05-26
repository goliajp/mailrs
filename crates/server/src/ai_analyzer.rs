use std::sync::Arc;

use tokio::sync::mpsc;

use mailrs_intelligence::analyze;
use mailrs_intelligence::provider::LlmProvider;
use mailrs_mailbox::{EmailAnalysisInput, PgMailboxStore};

use crate::event_bus::{EventBus, SmtpEvent};
use crate::message_util;

type Job = (i64, String, String, String, String); // (msg_id, user, maildir_id, sender, subject)

/// spawn the email analyzer — single serial queue, new emails pushed to front
pub fn spawn_analyzer(
    provider: Arc<dyn LlmProvider>,
    mailbox_store: Arc<PgMailboxStore>,
    event_bus: EventBus,
    maildir_root: String,
) {
    let (tx, rx) = mpsc::unbounded_channel::<Job>();

    // listener: enqueue new emails as they arrive
    {
        let store = mailbox_store.clone();
        let mv = provider.model_id().to_string();
        let tx = tx.clone();
        tokio::spawn(async move { listen_new_messages(store, event_bus, tx, mv).await });
    }

    // single worker: drain new emails first, then pick backfill
    tokio::spawn(async move {
        worker(provider, mailbox_store, maildir_root, rx).await;
    });

    tracing::info!(event = "subsystem_started", subsystem = "ai_analyzer");
}

/// single serial worker
async fn worker(
    provider: Arc<dyn LlmProvider>,
    store: Arc<PgMailboxStore>,
    maildir_root: String,
    mut new_rx: mpsc::UnboundedReceiver<Job>,
) {
    let model_version = provider.model_id().to_string();
    let mut done = 0u64;
    let mut failed = 0u64;
    let mut consecutive_fails = 0u32;

    let total = store
        .count_unanalyzed_messages(&model_version)
        .await
        .unwrap_or(0);
    if total > 0 {
        tracing::info!(event = "ai_backfill_started", total, model_version = %model_version);
    } else {
        tracing::info!(event = "ai_backfill_uptodate", model_version = %model_version);
    }

    loop {
        // priority 1: drain all pending new emails
        while let Ok(job) = new_rx.try_recv() {
            process_one(
                &*provider,
                &store,
                &maildir_root,
                job,
                &model_version,
                &mut done,
                &mut failed,
                &mut consecutive_fails,
                total,
            )
            .await;
        }

        // priority 2: backfill — serial, one at a time
        let batch = store
            .list_unanalyzed_message_ids(1, &model_version)
            .await
            .unwrap_or_default();
        if let Some(job) = batch.into_iter().next() {
            process_one(
                &*provider,
                &store,
                &maildir_root,
                job,
                &model_version,
                &mut done,
                &mut failed,
                &mut consecutive_fails,
                total,
            )
            .await;
        } else {
            // backfill done — wait for new email
            if done > 0 || failed > 0 {
                tracing::info!(event = "ai_backfill_complete", done, failed);
                done = 0;
                failed = 0;
            }
            match new_rx.recv().await {
                Some(job) => {
                    process_one(
                        &*provider,
                        &store,
                        &maildir_root,
                        job,
                        &model_version,
                        &mut done,
                        &mut failed,
                        &mut consecutive_fails,
                        total,
                    )
                    .await;
                }
                None => break,
            }
        }
    }
}

/// process one message with retry + backoff tracking
#[allow(clippy::too_many_arguments)]
async fn process_one(
    provider: &dyn LlmProvider,
    store: &PgMailboxStore,
    maildir_root: &str,
    (msg_id, user, maildir_id, sender, subject): Job,
    model_version: &str,
    done: &mut u64,
    failed: &mut u64,
    consecutive_fails: &mut u32,
    total: i64,
) {
    let success = analyze_with_retry(
        provider,
        store,
        maildir_root,
        msg_id,
        &user,
        &maildir_id,
        &sender,
        &subject,
        model_version,
    )
    .await;

    if success {
        *done += 1;
        *consecutive_fails = 0;
        if (*done).is_multiple_of(20) {
            tracing::info!(event = "ai_backfill_progress", done, total, failed);
        }
    } else {
        *failed += 1;
        *consecutive_fails += 1;
        // exponential backoff: 30s → 60s → 120s → ... → 3600s (1h) cap
        let wait = (30u64 << (*consecutive_fails).saturating_sub(1).min(6)).min(3600);
        tracing::warn!(
            event = "ai_backfill_backoff",
            consecutive_fails = *consecutive_fails,
            wait_secs = wait
        );
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
    }
}

/// listen for new messages and enqueue
async fn listen_new_messages(
    store: Arc<PgMailboxStore>,
    event_bus: EventBus,
    tx: mpsc::UnboundedSender<Job>,
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

/// analyze with up to 2 retries
#[allow(clippy::too_many_arguments)]
async fn analyze_with_retry(
    provider: &dyn LlmProvider,
    store: &PgMailboxStore,
    maildir_root: &str,
    message_id: i64,
    user: &str,
    maildir_id: &str,
    sender_raw: &str,
    subject_raw: &str,
    model_version: &str,
) -> bool {
    for attempt in 0..2u32 {
        if attempt > 0 {
            let delay = if attempt == 1 { 15 } else { 30 };
            tracing::debug!(
                event = "ai_retry",
                message_id,
                attempt = attempt + 1,
                backoff_secs = delay
            );
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
        if do_analyze(
            provider,
            store,
            maildir_root,
            message_id,
            user,
            maildir_id,
            sender_raw,
            subject_raw,
            model_version,
        )
        .await
        {
            return true;
        }
    }
    tracing::warn!(event = "ai_analyze_gave_up", message_id, attempts = 2);
    false
}

#[allow(clippy::too_many_arguments)]
async fn do_analyze(
    provider: &dyn LlmProvider,
    store: &PgMailboxStore,
    maildir_root: &str,
    message_id: i64,
    user: &str,
    maildir_id: &str,
    sender_raw: &str,
    subject_raw: &str,
    model_version: &str,
) -> bool {
    let raw = match message_util::read_message_raw(maildir_root, user, maildir_id).await {
        Some(r) => r,
        None => return false,
    };

    let (text_body, html_body, _) = message_util::parse_message(&raw);
    let sender = message_util::decode_header(sender_raw);
    let subject = message_util::decode_header(subject_raw);
    let body_text = text_body.as_deref().or(html_body.as_deref()).unwrap_or("");

    let attachment_text = store
        .get_attachment_texts(message_id)
        .await
        .unwrap_or_default();
    let body = if attachment_text.is_empty() {
        body_text.to_string()
    } else {
        format!("{body_text}\n\n[Attachment content]\n{attachment_text}")
    };

    // analysis first, then embedding (separate models, serial)
    let t0 = std::time::Instant::now();
    let analysis = match analyze::analyze_email(provider, &sender, &subject, &body).await {
        Some(a) => a,
        None => {
            tracing::debug!(event = "ai_analyze_no_result", message_id);
            return false;
        }
    };
    let t_analysis = t0.elapsed();

    let t1 = std::time::Instant::now();
    let embedding_text = format!("{subject}\n\n{body}");
    let embedding = provider.embed(&embedding_text).await;
    let t_embed = t1.elapsed();

    let intent = if analysis.sender_intent.is_empty() {
        "inform"
    } else {
        &analysis.sender_intent
    };

    // validate action_deadline — LLM sometimes returns non-timestamp text like "30 分钟内"
    let deadline = analysis.action_deadline.as_deref().filter(|d| {
        chrono::DateTime::parse_from_rfc3339(d).is_ok()
            || chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok()
    });

    let input = EmailAnalysisInput {
        message_id,
        category: &analysis.category,
        risk_score: analysis.risk_score as i16,
        risk_reason: &analysis.risk_reason,
        summary: &analysis.summary,
        people: &analysis.people,
        dates: &analysis.dates,
        amounts: &analysis.amounts,
        action_items: &analysis.action_items,
        embedding: embedding.as_deref(),
        model_version,
        clean_text: &analysis.clean_text,
        requires_action: analysis.requires_action,
        sender_intent: intent,
        action_deadline: deadline,
    };
    if let Err(e) = store.upsert_email_analysis(&input).await {
        tracing::error!(event = "ai_analyze_db_error", message_id, error = %e);
        return false;
    }

    if analysis.requires_action {
        let _ = store.boost_importance_for_action(message_id).await;
    }

    tracing::info!(
        event = "ai_analyzed",
        message_id,
        category = %analysis.category,
        risk_score = analysis.risk_score,
        requires_action = analysis.requires_action,
        intent = %intent,
        has_embedding = embedding.is_some(),
        analysis_secs = t_analysis.as_secs_f64(),
        embed_secs = t_embed.as_secs_f64()
    );
    true
}
