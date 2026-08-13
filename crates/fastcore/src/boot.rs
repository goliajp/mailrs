//! Bringing the process up: listeners, background tasks, and the shutdown
//! that has to finish before the kevy AOF is safe to replay.
//!
//! The graceful stop is not decoration. A hard kill mid-write leaves a
//! torn frame at the AOF tail; replay stops there while appends continue
//! past it, so every restart rolls the store back to the moment of the
//! tear and new writes vanish. That is what
//! `.claude/rules/dev-deploy-workflow.md` means by "部署后必看 replay
//! 日志是否 (clean)".

use std::sync::Arc;

use crate::*;

/// Log the store's boot verdict and say whether it was intact.
///
/// `Store::open_report` is kevy 4.0's answer to a failure mode we
/// reported: a corrupt AOF frame silently truncated replay, every
/// subsequent write landed past the stop point and was dropped again,
/// and the only evidence was a line on stderr nobody read. The verdict
/// is data now, so it can gate a deploy.
///
/// Intact means the replay reached the end of what the files held:
/// nothing corrupt, nothing dropped. `resynced_bytes` is reported but
/// does not by itself mean damage — under `replay_resync` it counts
/// bytes hopped over to recover a good tail, which is a better outcome
/// than surrendering it.
fn report_boot(store: &Store) -> bool {
    let r = store.open_report();
    let intact = !r.corrupt && r.dropped_bytes == 0;
    if intact {
        tracing::info!(
            replayed_commands = r.replayed_commands,
            replayed_bytes = r.replayed_bytes,
            elapsed_ms = r.elapsed_ms,
            "kevy boot report: intact"
        );
    } else {
        tracing::error!(
            replayed_commands = r.replayed_commands,
            replayed_bytes = r.replayed_bytes,
            dropped_bytes = r.dropped_bytes,
            corrupt = r.corrupt,
            resynced_bytes = r.resynced_bytes,
            quarantine = ?r.quarantine_paths,
            "kevy boot report: DAMAGED — health check will report not-ready; \
             the quarantined bytes are preserved, do not restart over this"
        );
    }
    intact
}

pub async fn run() {
    // Install the process-wide rustls crypto provider before any TLS
    // config is built (IMAPS / POP3S acceptors, ACME challenge server).
    // Without this rustls 0.23 panics on first use — same fix
    // mailrs-receiver / mailrs-fastcore-sender apply. `.ok()` because
    // a second install is a no-op error we can safely ignore.
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let kevy_dir =
        std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_else(|_| "/data/kevy-fastcore".to_string());
    // v2 Stage B.8: enable the kevy 3.17 change feed so IMAP IDLE (and
    // future JMAP push, WS bridges) can subscribe via changes_since
    // instead of the in-memory tokio broadcast channel. The feed is
    // durable across restarts (offset resumes) and buffers writes so
    // a slow consumer doesn't lose events. 16 MiB buffer ≈ 250K
    // change frames (~64 B each) — plenty for a per-user IDLE
    // consumer under normal load.
    let mut cfg = Config::default()
        .with_persist(&kevy_dir)
        .with_feed(16 * 1024 * 1024)
        // Recover the good tail behind a corrupt region instead of
        // surrendering everything after it. Strict replay stops at the
        // first bad frame — which is precisely how three days of writes
        // went missing here in 2026-07: the damage was an 8-byte splice
        // mid-file, and everything past it was intact. kevy's crashgate
        // replays that exact shape and recovers 100500/100500 records.
        //
        // A boundary is trusted only when length, CRC and a well-formed
        // single-command parse all agree, and skipped ranges are still
        // reported with the corrupt flag raised — so this recovers data
        // without hiding that anything happened.
        .with_replay_resync(true);
    // Transparent tiering. Off by default: the whole dataset fits in
    // RAM today, and a budget below the working set trades latency for
    // memory in a way that has to be measured before it is imposed.
    //
    // MAILRS_KEVY_TIER_MB=<n> sets an explicit budget; `auto` takes
    // 70% of the detected cgroup limit. Cold values move to a vlog on
    // disk and read back transparently, so this changes footprint and
    // tail latency, never answers.
    if let Ok(spec) = std::env::var("MAILRS_KEVY_TIER_MB") {
        cfg = match spec.as_str() {
            "auto" => cfg.with_tier_budget_auto(),
            n => match n.parse::<u64>() {
                Ok(mb) => cfg.with_tier_budget(mb * 1024 * 1024),
                Err(_) => {
                    tracing::warn!(%spec, "MAILRS_KEVY_TIER_MB unparseable — tiering stays off");
                    cfg
                }
            },
        };
        tracing::info!(%spec, "kevy transparent tiering enabled");
    }
    // kevy 4.0 canary window. The AOF record format is new in 4.0
    // (KEVYAOF2, length-prefixed + CRC32C), and the upgrade happens on
    // the first rewrite — one-way, per kevy's UPGRADING. Appends to an
    // existing v1 file stay v1, so while rewrite is off a downgrade to
    // 3.18 is still just a binary swap back.
    //
    // Set MAILRS_KEVY_AOF_REWRITE=off during the canary; unset it to
    // let the three-trigger policy run and convert the file.
    if std::env::var("MAILRS_KEVY_AOF_REWRITE").as_deref() == Ok("off") {
        cfg = cfg.with_auto_aof_rewrite(0, u64::MAX);
        tracing::warn!(
            "kevy AOF auto-rewrite DISABLED (canary) — file stays v1, \
             downgrade to 3.18 remains possible; unset \
             MAILRS_KEVY_AOF_REWRITE to re-enable"
        );
    }
    let store = Arc::new(Store::open(cfg).expect("open kevy store"));
    let boot_intact = report_boot(&store);
    let mailbox = KevyMailboxStore::new(store);
    // v2.6.0 §P6: register the admin-CRUD range indexes idempotently.
    mailbox.ensure_admin_indexes();
    // v4 TABLE layer: declare the access paths the engine maintains.
    // These ARE the read path — `*_via_table` is what every list and count
    // goes through. The per-user zsets they replaced are legacy: nothing
    // writes them, `tests/legacy_zset_readers.rs` fails if anything reads
    // one, and `maintenance:drop-legacy-zsets` removes what is left of them.
    // (Until 2026-08-01 this comment still said the zsets were authoritative
    // and nothing read the table, which had been false since the cutover.)
    mailbox.ensure_thread_table();

    // Alias-store backend selector — RFC 20260705 Step 2.
    // Default (`embed` / unset): historical fastcore-owned alias table
    // in the local kevy AOF. Cutover flip: `MAILRS_ALIAS_STORE_BACKEND=network`
    // + `MAILRS_KEVY_URL=…` moves the source of truth into the shared
    // network kevy so pg-core / monolith read the same rows during
    // stack switches — no per-cutover dump/load needed.
    let alias_backend =
        std::env::var("MAILRS_ALIAS_STORE_BACKEND").unwrap_or_else(|_| "embed".into());
    let alias_store: Arc<dyn AliasStore> = match alias_backend.as_str() {
        "network" => {
            let url = std::env::var("MAILRS_KEVY_URL").expect(
                "MAILRS_ALIAS_STORE_BACKEND=network requires MAILRS_KEVY_URL to point at the shared kevy",
            );
            tracing::info!(url = %url, "alias-store backend = network kevy");
            let store = mailrs_alias_store_net::NetworkKevyAliasStore::new(url);
            // v2.6.0 §P6 dual-write: declare the network-side alias
            // range indexes idempotently. Best-effort — network kevy
            // may momentarily be unavailable at boot; the next writer
            // will retry the declaration on any subsequent upsert /
            // ensure call.
            if let Err(e) = store.ensure_indexes() {
                tracing::warn!(error = %e, "alias-store network idx_create failed at boot");
            }
            Arc::new(store)
        }
        _ => {
            tracing::info!("alias-store backend = embed kevy (default)");
            Arc::new(mailbox.clone())
        }
    };
    let state = Arc::new(
        FastcoreState::new_with_alias_store(mailbox, alias_store).with_boot_intact(boot_intact),
    );
    // Held for the shutdown path — the router takes ownership of the
    // original below.
    let shutdown_state = state.clone();

    // Spawn the ingestion sync loop before the HTTP listener so new
    // messages start replicating as soon as the process boots. Failures
    // are logged + retried; they don't crash the server.
    let sync_state = state.clone();
    tokio::spawn(async move {
        ingest_sync_loop(sync_state).await;
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

    // v2.4.2 Phase 4.2 (RFC-C §4.2): Junk-folder retention sweep.
    // Runs every 24h; expunges Junk-zset entries whose latest_date
    // is older than the per-user TTL (default 30 days).
    junk_ttl::spawn(state.clone());
    snooze_wake::spawn(state.clone());
    // Say now whether push is on, rather than at the first delivery.
    push::warm();
    aof_compact::spawn(state.clone(), kevy_dir.clone());

    // ACME renewal task. Reads MAILRS_ACME_EMAIL/DOMAINS; noop if
    // either is unset. Binds port 80 for the HTTP-01 challenge server
    // and periodically renews the cert to `MAILRS_ACME_DIR`. Receiver
    // + webapi consume the resulting cert files on their own reload
    // cadence — fastcore doesn't serve TLS itself.
    tokio::spawn(async move {
        acme_task::spawn().await;
    });

    // IMAP + IMAPS + POP3 + POP3S listeners. Cert comes from
    // MAILRS_TLS_CERT + MAILRS_TLS_KEY (same paths the receiver uses)
    // — matching the monolith's TLS pattern: plaintext port loads no
    // cert, implicit-TLS port wraps every accepted socket via a
    // shared rustls acceptor before entering the session. Set each
    // MAILRS_(IMAP|IMAPS|POP3|POP3S)_BIND=off to disable per-port.
    let imap_state = state.clone();
    tokio::spawn(async move {
        imap::spawn(imap_state).await;
    });
    let imaps_state = state.clone();
    tokio::spawn(async move {
        imap::spawn_tls(imaps_state).await;
    });
    let pop3_state = state.clone();
    tokio::spawn(async move {
        pop3::spawn(pop3_state).await;
    });
    let pop3s_state = state.clone();
    tokio::spawn(async move {
        pop3::spawn_tls(pop3s_state).await;
    });
    // Webhook delivery — drains the kevy outbox. Until 2026-07-31 a
    // subscription could be created and nothing would ever fire on this
    // lane; there was no queue and no worker.
    let webhook_state = state.clone();
    tokio::spawn(async move {
        webhook_delivery::spawn(webhook_state).await;
    });
    // Calendar feed sync — subscribing to an .ics URL stored a row that
    // nothing read, so no event ever appeared.
    let calendar_state = state.clone();
    tokio::spawn(async move {
        calendar_sync::spawn(calendar_state).await;
    });
    // ManageSieve (RFC 5804) — sieve script CRUD on :4190 (G5).
    let sieve_state = state.clone();
    tokio::spawn(async move {
        managesieve::spawn(sieve_state).await;
    });

    let addr = std::env::var("MAILRS_FASTCORE_BIND").unwrap_or_else(|_| "0.0.0.0:3301".into());

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind MAILRS_FASTCORE_BIND");
    tracing::info!(
        addr = %addr,
        kevy_dir = %kevy_dir,
        "mailrs-fastcore listening (kevy backend)"
    );
    // Exit gracefully on SIGTERM/SIGINT instead of letting the default
    // handler kill the process mid-write. Returning from run() drops the
    // runtime → every task's Arc<Store> releases → kevy's DropGuard
    // flushes each shard's AOF. Without this, `docker stop` (every
    // deploy) tore a half-written frame into the AOF tail and the next
    // boot's replay DROPPED everything after it — 181 MB / several days
    // of writes on 2026-07-17 (vanished mail, resurrected threading
    // fragments).
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        r = axum::serve(listener, app) => {
            r.unwrap();
        }
        _ = sigterm.recv() => {
            tracing::info!("SIGTERM — flushing kevy before exit");
            flush_kevy(&shutdown_state);
            exit_after_flush();
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("SIGINT — flushing kevy before exit");
            flush_kevy(&shutdown_state);
            exit_after_flush();
        }
    }
}

/// Leave, now that the store is sealed.
///
/// Returning from here instead would drop the runtime, and dropping a
/// runtime waits for its blocking tasks — of which this process has
/// several that loop forever on a bounded `BRPOP`, by design. So the
/// process flushed, said so, and then never exited: measured still alive
/// 40 s after SIGTERM whenever `MAILRS_KEVY_URL` was set, which is the
/// production configuration. Every deploy was waiting out `docker stop`'s
/// grace period and taking a SIGKILL.
///
/// This is safe precisely because `flush_kevy` ran first: kevy's
/// `shutdown()` fsyncs every shard's AOF and then refuses further writes
/// with `KevyError::Closed`, so there is no write left in flight for a
/// skipped destructor to lose. Maildir writes are closed files. The
/// network kevy is somebody else's process.
fn exit_after_flush() -> ! {
    tracing::info!("exiting");
    std::process::exit(0);
}

/// Flush and seal the store on the way out.
///
/// The previous version relied on returning from `run()` dropping the
/// runtime, which released every task's `Arc<Store>`, which let kevy's
/// DropGuard flush. That works only if nothing else still holds a
/// clone — a race the code could not actually prove it won.
///
/// kevy 4.0 makes it explicit: `shutdown()` fsyncs everything and then
/// refuses further writes with `KevyError::Closed`, and it is
/// clone-safe, so it does not matter who else is holding the store.
/// 4.0 also force-fsyncs the AOF tail before writing the feed's
/// clean-shutdown marker — the marker could previously claim a
/// durability the tail did not have.
fn flush_kevy(state: &Arc<FastcoreState>) {
    match state.mailbox.store_ref().shutdown() {
        Ok(()) => tracing::info!("kevy shutdown complete — AOF fsynced, writes refused"),
        Err(e) => tracing::error!(error = %e, "kevy shutdown failed; AOF tail may be unflushed"),
    }
}
