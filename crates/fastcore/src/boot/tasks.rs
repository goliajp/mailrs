//! The background tasks and listeners the process starts after the store is open.

use std::sync::Arc;

use crate::*;

/// The ingest, drain, retention and renewal tasks.
pub(super) fn spawn_workers(state: &Arc<FastcoreState>, kevy_dir: &str) {
    // Spawn the ingestion sync loop before the HTTP listener so new
    // messages start replicating as soon as the process boots. Failures
    // are logged + retried; they don't crash the server.
    let sync_state = state.clone();
    tokio::spawn(async move {
        crate::ingest_loop::ingest_sync_loop(sync_state).await;
    });

    // Spawn the spool drain — receiver writes {spool}/incoming/new/*
    // in split topology, and nothing else consumes it. Missing this
    // task is what causes inbound Gmail / GitHub / etc. to sit in the
    // spool forever ("Sender said 250 OK, user never sees it"). See
    // `spool_drain.rs`.
    let drain_state = state.clone();
    tokio::spawn(async move {
        spool_drain::spawn(drain_state).await;
    });

    // Bounce DSN hand-off queue (G9): the sender enqueues composed
    // DSNs; we deliver them into the local sender's maildir + ingest.
    bounce::spawn_bounce_drain(state.clone());

    // TLS-RPT daily aggregate submission (G8.3).
    tlsrpt::spawn_submit(state.clone());

    // retention sweep: mock mail past 3 days
    crate::mock_sweep::spawn(state.clone());
    snooze_wake::spawn(state.clone());
    crate::fraud_backfill::spawn(state.clone(), kevy_dir);
    crate::attachment_text::spawn(state.clone());
    crate::image_prefetch::spawn(state.clone());
    crate::routes::trash::spawn_purge(state.clone());
    // Say now whether push is on, rather than at the first delivery.
    push::warm();
    aof_compact::spawn(state.clone(), kevy_dir.to_string());

    // ACME renewal task. Reads MAILRS_ACME_EMAIL/DOMAINS; noop if
    // either is unset. Binds port 80 for the HTTP-01 challenge server
    // and periodically renews the cert to `MAILRS_ACME_DIR`. Receiver
    // + webapi consume the resulting cert files on their own reload
    // cadence — fastcore doesn't serve TLS itself.
    tokio::spawn(acme_task::spawn());
}

/// IMAP, POP3 and ManageSieve, plain and wrapped.
pub(super) fn spawn_listeners(state: &Arc<FastcoreState>) {
    // IMAP + IMAPS + POP3 + POP3S listeners share one certificate from
    // MAILRS_TLS_CERT + MAILRS_TLS_KEY, followed on disk: 993/995 wrap
    // every socket, 143/110 offer STARTTLS / STLS. Set each
    // MAILRS_(IMAP|IMAPS|POP3|POP3S)_BIND=off to disable per-port.
    // ManageSieve (RFC 5804) on :4190 offers STARTTLS from the same
    // certificate.
    crate::protocols::spawn(state.clone(), crate::tls::from_env());
}

/// Webhook delivery and calendar feed sync.
pub(super) fn spawn_outbound_feeds(state: &Arc<FastcoreState>) {
    // Webhook delivery — drains the kevy outbox. Until 2026-07-31 a
    // subscription could be created and nothing would ever fire on this
    // lane; there was no queue and no worker.
    {
        let state = state.clone();
        tokio::spawn(mailrs_intake::webhook_delivery::run(std::sync::Arc::new(
            move || state.net_conn(),
        )));
    }
    // Calendar feed sync — subscribing to an .ics URL stored a row that
    // nothing read, so no event ever appeared.
    let calendar_state = state.clone();
    tokio::spawn(async move {
        calendar_sync::spawn(calendar_state).await;
    });
}
