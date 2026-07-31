//! `mailrs-fastcore` — Kevy-backed implementation of the
//! `mailrs-core-api` server surface. Phase 8.
//!
//! Today this binary mounts a small subset:
//! - `/v1/healthz` + `/v1/readyz` (open) — proves the role works
//! - `POST /v1/users/{user}/conversations:list` — Rock 1 read path
//!
//! The rest of the 87-route surface fills in as `mailbox-kevy` grows
//! method coverage. Run alongside (or instead of) the monolith core
//! to A/B test conversation-list latency under the same load.
//!
//! Environment:
//! - `MAILRS_FASTCORE_BIND` — listen address (default `0.0.0.0:3301`,
//!   one above the monolith's core-rpc :3300 so both can coexist)
//! - `MAILRS_KEVY_DATA_DIR` — kevy persist dir (default
//!   `/data/kevy-fastcore`)

#![allow(missing_docs)]

mod acme_task;
mod aof_compact;
pub mod arc_seal;
mod backfill_decode;
mod bayes_train;
pub mod bounce;
pub mod dmarc_ingest;
pub mod fbl;
mod imap;
mod importance;
mod junk_ttl;
pub mod live_sync;
mod managesieve;
mod pop3;
mod routes;
pub mod sender_sts;
mod sieve_apply;
mod spool_drain;
pub mod tlsrpt;
pub mod tlsrpt_ingest;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post, put};
use kevy_embedded::{Config, Store};
use mailrs_alias_store::AliasStore;
use mailrs_core_api::method::admin as adm;
use mailrs_core_api::method::analysis as an;
use mailrs_core_api::method::contact as ct;
use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::mailbox as mb;
use mailrs_core_api::method::message as msg;
use mailrs_core_api::method::outbound as ob;
use mailrs_core_api::method::thread as th;
use mailrs_core_api::server::{Handler, base_router};
use mailrs_core_api::types::{BackendKind, ConversationSummaryWire, HealthResponse};
use mailrs_mailbox_kevy::{KevyMailboxStore, ListThreadsFilter, ThreadRow};

/// Server state — owns the kevy store and is cloned into axum handlers.
pub struct FastcoreState {
    pub mailbox: KevyMailboxStore,
    /// Alias resolver / admin. Backend-agnostic: fastcore's boot code
    /// currently constructs an `Arc<KevyMailboxStore>` here (embedded
    /// kevy), but any [`AliasStore`] impl works — the planned
    /// network-kevy backend (RFC 20260705) drops in without touching
    /// call sites. Handlers hold `state.clone()`, so `Arc` is required.
    pub alias_store: std::sync::Arc<dyn AliasStore>,
    /// In-process delivery fanout: every write path publishes the
    /// recipient address here; IMAP IDLE sessions subscribe and push
    /// `* n EXISTS` to their client (RFC 2177). Drain + RPC + IMAP all
    /// live in this process, so no kevy pub/sub hop is needed.
    pub notify: tokio::sync::broadcast::Sender<String>,
    /// False when the store's boot report showed a damaged AOF.
    ///
    /// kevy 4.0 turns the boot verdict into data (`Store::open_report`)
    /// rather than a line on stderr. That exists because of our own
    /// incident: a corrupt frame black-holed three days of writes while
    /// every restart looked normal. Surfacing it here means a deploy
    /// over a damaged boot cannot go green — the container keeps
    /// serving mail (a live-but-unhealthy instance still delivers; a
    /// dead one does not) but the health check refuses.
    pub boot_intact: bool,
    /// Network-kevy URL (`MAILRS_KEVY_URL`) for the shared side-state
    /// routes (drafts / signatures / templates / reactions / webhooks /
    /// audit / outbound / groups). These live in the INDEPENDENT network
    /// kevy — the same keys webapi + the pg-core read — so both cores
    /// serve them identically. `None` in tests / when unset: side-state
    /// routes return empty results rather than erroring.
    pub net_url: Option<String>,
}

impl FastcoreState {
    /// Construct state with a fresh notify channel. Reads the network-kevy
    /// URL from `MAILRS_KEVY_URL` (absent in tests → side-state disabled).
    /// Alias store defaults to the embedded-kevy backend backed by the
    /// same `mailbox` handle; swap in a network-kevy impl at the boot
    /// site when RFC 20260705 Step 2 lands.
    pub fn new(mailbox: KevyMailboxStore) -> Self {
        let alias_store: std::sync::Arc<dyn AliasStore> = std::sync::Arc::new(mailbox.clone());
        Self::new_with_alias_store(mailbox, alias_store)
    }

    /// Construct with an explicit alias-store backend. Used by tests and
    /// by the planned network-kevy boot path; the default constructor
    /// wires the embedded-kevy impl for backwards compatibility.
    pub fn new_with_alias_store(
        mailbox: KevyMailboxStore,
        alias_store: std::sync::Arc<dyn AliasStore>,
    ) -> Self {
        let (notify, _) = tokio::sync::broadcast::channel(256);
        let net_url = std::env::var("MAILRS_KEVY_URL")
            .ok()
            .filter(|s| !s.is_empty());
        Self {
            mailbox,
            alias_store,
            notify,
            net_url,
            boot_intact: true,
        }
    }

    /// Record the store's boot verdict. Called once at startup with the
    /// result of `Store::open_report`; see [`Self::boot_intact`].
    pub fn with_boot_intact(mut self, intact: bool) -> Self {
        self.boot_intact = intact;
        self
    }

    /// Open a fresh network-kevy connection for a side-state handler.
    /// Follows the per-use `Connection::open` pattern the auxiliary tasks
    /// use (spool_drain / live_sync / sieve_apply). Returns `None` when no
    /// network kevy is configured so handlers can serve an empty result.
    pub fn net_conn(&self) -> Option<kevy_client::Connection> {
        let url = self.net_url.as_ref()?;
        kevy_client::Connection::connect(url).ok()
    }
}

impl mailrs_core_sidestate::NetKevy for FastcoreState {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        FastcoreState::net_conn(self)
    }
}

impl Handler for FastcoreState {
    async fn healthz(&self) -> HealthResponse {
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: self.boot_intact,
        }
    }

    async fn readyz(&self) -> HealthResponse {
        // kevy is in-process, so the store is up whenever the binary
        // is — but "up" is not "intact". A boot that dropped bytes is
        // serving a keyspace smaller than the files held, and that must
        // not read as ready.
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: self.boot_intact,
        }
    }
}

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
    // Nothing reads them yet — the per-user zsets stay authoritative
    // until the shadow-read window says the two agree.
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
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("SIGINT — flushing kevy before exit");
            flush_kevy(&shutdown_state);
        }
    }
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

/// Periodic sync loop. Two jobs on the same tick:
///
/// 1. OPTIONAL ingest: when `MAILRS_CORE_RPC_BASE` + the shared secret
///    are set, poll that core-api server for threads newer than the
///    per-user cursor and mirror them in (the monolith-era cutover
///    path).
/// 2. MANDATORY maildir self-heal: thread/message/uid repair straight
///    from disk. This must run regardless of the ingest config —
///    returning early when MAILRS_CORE_RPC_BASE was unset silently
///    killed self-heal on the first monolith-free deploy and new
///    inbound mail stopped appearing in the UI (2026-07-04, 99-message
///    backlog on prod before the stopgap).
async fn ingest_sync_loop(state: Arc<FastcoreState>) {
    let client = match (
        std::env::var("MAILRS_CORE_RPC_BASE"),
        std::env::var(mailrs_core_api::AUTH_SECRET_ENV),
    ) {
        (Ok(base), Ok(secret)) => Some(mailrs_core_api::client::Client::new(base, secret)),
        _ => {
            tracing::info!("no ingest source configured — running maildir self-heal only");
            None
        }
    };
    let interval = std::time::Duration::from_secs(
        std::env::var("MAILRS_FASTCORE_SYNC_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30),
    );
    // Self-heal pacing. Every writer of a maildir file now indexes it
    // write-through (spool_drain, bounce, IMAP APPEND/COPY, REST
    // copy/move), so the only gap left for this sweep is a process that
    // died between the file landing and the index write. Those files are
    // necessarily recent, so the routine pass only inspects names newer
    // than INCREMENTAL_WINDOW and costs a readdir instead of ~48k header
    // reads (staging 2026-07-19).
    //
    // A full pass still runs at boot and once a day, to catch anything a
    // clock skew or an out-of-band file drop put outside the window.
    // Backoff on top: each idle round doubles the wait, any repair
    // resets it to the base interval.
    const MAX_IDLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(900);
    const FULL_SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    // Generous relative to the crash window it covers; the cost of a
    // wider window is only extra header reads on files we then skip.
    const INCREMENTAL_WINDOW_SECS: i64 = 6 * 3600;
    let mut idle_rounds: u32 = 0;
    let mut last_full_sweep: Option<std::time::Instant> = None;
    loop {
        let mut wait = interval;
        match &client {
            Some(c) => {
                if let Err(e) = run_ingest_once(&state, c).await {
                    tracing::warn!(error = %e, "ingest sync tick failed");
                }
            }
            None => {
                let full = last_full_sweep.is_none_or(|t| t.elapsed() >= FULL_SWEEP_EVERY);
                let since = match full {
                    true => 0,
                    false => now_secs().saturating_sub(INCREMENTAL_WINDOW_SECS),
                };
                let addrs = state.mailbox.list_account_addresses().unwrap_or_default();
                let mut repaired = false;
                for user in &addrs {
                    repaired |= healed_from_maildir(&state, user, since).await;
                }
                if full {
                    last_full_sweep = Some(std::time::Instant::now());
                }
                if repaired {
                    idle_rounds = 0;
                } else {
                    idle_rounds = idle_rounds.saturating_add(1);
                }
                let backoff = interval
                    .saturating_mul(1u32 << idle_rounds.min(6))
                    .min(MAX_IDLE_INTERVAL);
                wait = backoff;
            }
        }
        tokio::time::sleep(wait).await;
    }
}

async fn run_ingest_once(
    state: &Arc<FastcoreState>,
    client: &mailrs_core_api::client::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    use mailrs_core_api::method::conversation::ListConversationsRequest;
    use mailrs_core_api::types::ConversationFilter;

    let addrs = state.mailbox.list_account_addresses()?;
    for user in &addrs {
        let cursor_key = format!("mailrs:sync:cursor:{user}");
        let prev = state
            .mailbox
            .store_ref()
            .get(cursor_key.as_bytes())?
            .and_then(|b| String::from_utf8_lossy(&b).parse::<i64>().ok())
            .unwrap_or(0);
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                limit: 200,
                before_ts: None,
                category: None,
                domains: None,
                archived: false,
                folder: None,
                unread: None,
                starred: None,
                section: None,
            },
        };
        // Try monolith. If it's down, skip the ingest step but STILL
        // run the maildir-based self-heal at the bottom of the loop —
        // fastcore's whole point is to work without monolith.
        let resp_opt = match client.list_conversations(user, &req).await {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(error = %e, %user, "monolith list_conversations failed (continuing to self-heal from maildir)");
                None
            }
        };
        let resp = match resp_opt {
            Some(r) => r,
            None => {
                // core RPC unavailable — full sweep, this path is rare
                healed_from_maildir(state, user, 0).await;
                continue;
            }
        };
        let mut max_seen = prev;
        let mut newly = 0;
        for s in &resp.items {
            if s.last_date <= prev {
                continue;
            }
            // If the thread already exists in kevy, don't clobber the
            // aggregate (fastcore-side mark_read / pin / archive stay
            // sticky) — but DO diff messages, because a thread with a
            // new reply gets its `last_date` bumped and needs the new
            // message body ingested. Prior version skipped the whole
            // packet, so new replies never appeared until the user
            // re-imported.
            let already_exists = matches!(state.mailbox.get_thread(&s.thread_id), Ok(Some(_)));
            if already_exists {
                if let Ok(msgs) = client.list_thread_messages(user, &s.thread_id).await {
                    for w in &msgs.items {
                        // Only write if we don't already have this
                        // message id (prevents duplicate writes on
                        // every sync tick).
                        if state
                            .mailbox
                            .get_message(&w.message_id)
                            .ok()
                            .flatten()
                            .is_some()
                        {
                            continue;
                        }
                        let payload = match serde_json::to_vec(w) {
                            Ok(p) => p,
                            Err(_) => continue,
                        };
                        let _ = state.mailbox.upsert_message(
                            &s.thread_id,
                            &w.message_id,
                            w.internal_date,
                            &payload,
                        );
                        let _ = state.mailbox.index_uid(user, w.uid, &w.message_id);
                    }
                }
                max_seen = max_seen.max(s.last_date);
                continue;
            }
            let row = mailrs_mailbox_kevy::ThreadRow {
                thread_id: s.thread_id.clone(),
                subject: s.subject.clone(),
                senders_csv: s.participants.clone(),
                count: s.message_count as i64,
                unread_count: s.unread_count as i64,
                latest_date: s.last_date,
                latest_preview: s.snippet.clone(),
                category: s.category.clone(),
                importance_level: s.importance_level.clone(),
                importance_score: s.importance_score as f64,
                requires_action: s.requires_action,
                pinned: s.pinned,
                archived: s.archived,
                has_action: s.requires_action,
                sent_count: s.sent_count as i64,
                starred: s.flagged,
            };
            if let Err(e) = state.mailbox.upsert_thread(user, &row) {
                tracing::warn!(error = %e, %user, tid = %s.thread_id, "upsert_thread failed");
                continue;
            }
            // Pull the thread's messages and mirror them so `get_thread_messages`
            // returns the fresh content on the next click.
            if let Ok(msgs) = client.list_thread_messages(user, &s.thread_id).await {
                for w in &msgs.items {
                    let payload = match serde_json::to_vec(w) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let _ = state.mailbox.upsert_message(
                        &s.thread_id,
                        &w.message_id,
                        w.internal_date,
                        &payload,
                    );
                    let _ = state.mailbox.index_uid(user, w.uid, &w.message_id);
                }
            }
            max_seen = max_seen.max(s.last_date);
            newly += 1;
        }
        if newly > 0 {
            state
                .mailbox
                .store_ref()
                .set(cursor_key.as_bytes(), max_seen.to_string().as_bytes())?;
            tracing::info!(%user, newly, cursor = max_seen, "ingest sync applied");
        }

        // Self-heal pass — reads maildir directly, no monolith call.
        //
        // Fastcore's whole reason for existing is to be spg-independent.
        // If we heal by calling monolith, then a spg outage takes
        // fastcore down with it — defeats the point. Instead, walk the
        // user's maildir(s), parse each file's headers, and upsert any
        // messages whose thread_id already exists in fastcore but has
        // an empty messages zset.
        healed_from_maildir(state, user, 0).await;
    }
    Ok(())
}

/// Extract common headers from an RFC 5322 message. Returns
/// `(message_id, in_reply_to, references, subject, date_epoch, from, to)`.
///
/// `references` is every Message-ID token of the References header,
/// oldest (root) first. Threading resolves against the msgid→thread
/// index via `resolve_thread_by_ancestry`; `references[0]` is only the
/// last-resort root guess (it is NOT stable across hops — remote MUAs
/// rewrite it, which fragmented conversations before v2.9.5).
/// Read the sender-authentication verdict from a message's own
/// `Authentication-Results` header, folded to a stable token. Empty
/// when the header is absent (e.g. mail that reached the maildir by a
/// path that didn't stamp it). This is the self-hosted "is this sender
/// who they claim to be" signal — pure auth results, no model.
pub(crate) fn extract_sender_trust(raw: &[u8]) -> String {
    let head = &raw[..raw.len().min(16 * 1024)];
    // Find the (possibly folded) Authentication-Results field. Headers
    // are ASCII field names; scan lines, unfolding continuations.
    let text = String::from_utf8_lossy(head);
    let mut value: Option<String> = None;
    let mut collecting = false;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if collecting {
            if line.starts_with(' ') || line.starts_with('\t') {
                value.as_mut().unwrap().push(' ');
                value.as_mut().unwrap().push_str(line.trim());
                continue;
            }
            break; // header ended
        }
        if let Some(rest) = line
            .strip_prefix("Authentication-Results:")
            .or_else(|| line.strip_prefix("authentication-results:"))
        {
            value = Some(rest.trim().to_string());
            collecting = true;
        }
    }
    let Some(v) = value else {
        return String::new();
    };
    let results = mailrs_inbound::parse_auth_results(&v);
    if results.is_empty() {
        return String::new();
    }
    mailrs_inbound::sender_trust(&results).as_str().to_string()
}

pub(crate) fn extract_headers(
    raw: &[u8],
) -> (String, String, Vec<String>, String, i64, String, String) {
    let mut message_id = String::new();
    let mut in_reply_to = String::new();
    let mut references: Vec<String> = Vec::new();
    let mut subject = String::new();
    let mut date_epoch: i64 = 0;
    let mut from = String::new();
    let mut to = String::new();

    // We only need headers; stop at the first blank line.
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let head = &raw[..head_end];
    let s = String::from_utf8_lossy(head);
    // Unfold headers (RFC 5322 §2.2.3 — a header continues onto the
    // next line if that line starts with WSP).
    let mut cur = String::new();
    let mut lines: Vec<String> = Vec::new();
    for line in s.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            cur.push(' ');
            cur.push_str(line.trim_start());
        } else {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    for l in &lines {
        let Some((name, val)) = l.split_once(':') else {
            continue;
        };
        let val = val.trim();
        match name.to_ascii_lowercase().as_str() {
            "message-id" => message_id = strip_angle(val),
            "in-reply-to" => in_reply_to = strip_angle(val),
            "references" => {
                // Every <...> token, oldest (root) first — the full chain
                // feeds the msgid→thread resolver, not just token 0.
                references = val
                    .split_whitespace()
                    .filter_map(|tok| {
                        let t = tok.trim_matches(|c: char| c == '<' || c == '>' || c == ',');
                        (!t.is_empty()).then(|| t.to_string())
                    })
                    .collect();
            }
            "subject" => subject = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            // display-name part of address headers is rfc2047-encoded by
            // many senders — decode here so stores never hold =?..?= runes
            "from" => from = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "to" => to = mailrs_rfc2047::decode(val.as_bytes()).into_owned(),
            "date" => date_epoch = parse_rfc5322_date(val).unwrap_or(0),
            _ => {}
        }
    }
    (
        message_id,
        in_reply_to,
        references,
        subject,
        date_epoch,
        from,
        to,
    )
}

/// Resolve which existing thread a message belongs to via the per-user
/// `Message-ID → thread_id` index. `None` = nothing known, caller falls
/// back to the legacy root rule. The message's OWN id is consulted
/// first — a message that was already ingested (and possibly moved by a
/// rethread merge) must land back in its current thread, or self-heal
/// re-creates the pre-merge fragment on every boot. Then nearest
/// ancestor wins: In-Reply-To, then References newest → oldest.
pub(crate) fn resolve_thread_by_ancestry(
    state: &Arc<FastcoreState>,
    user: &str,
    own_mid: &str,
    in_reply_to: &str,
    references: &[String],
    subject: &str,
) -> Option<String> {
    if !own_mid.is_empty()
        && let Ok(Some(tid)) = state.mailbox.thread_for_message_id(user, own_mid)
    {
        // own-id hits skip the subject gate: the message is already IN
        // that thread (re-ingest / self-heal), splitting it here would
        // fight the recorded state.
        return Some(tid);
    }
    let mut candidate: Option<String> = None;
    if !in_reply_to.is_empty()
        && let Ok(Some(tid)) = state.mailbox.thread_for_message_id(user, in_reply_to)
    {
        candidate = Some(tid);
    }
    if candidate.is_none() {
        for mid in references.iter().rev() {
            if let Ok(Some(tid)) = state.mailbox.thread_for_message_id(user, mid) {
                candidate = Some(tid);
                break;
            }
        }
    }
    // Gmail's subject rule: an ancestry match only joins the ancestor's
    // conversation when the normalized subjects agree. A reply that
    // changes topic ("annual closing" sent as a reply to the "withholding
    // tax" thread) is a NEW conversation — otherwise the old thread's
    // display flips to the user's own outbound subject and reads like a
    // sent mail sitting in the Inbox (2026-07-17 report).
    let tid = candidate?;
    let subj_norm = mailrs_mailbox_kevy::normalize_subject(subject);
    if subj_norm.is_empty() {
        return Some(tid);
    }
    match state.mailbox.get_thread(&tid) {
        Ok(Some(row)) => {
            if mailrs_mailbox_kevy::normalize_subject(&row.subject) == subj_norm {
                Some(tid)
            } else {
                None
            }
        }
        _ => Some(tid),
    }
}

fn strip_angle(v: &str) -> String {
    let t = v.trim();
    if let Some(inner) = t.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        inner.trim().to_string()
    } else {
        t.trim_matches(|c: char| c == '<' || c == '>').to_string()
    }
}

/// Very small RFC 5322 date parser: `Wed, 01 Jul 2026 12:34:56 +0000`.
/// Only accepts `+0000`/`-0000`-style offsets; that covers everything
/// modern MTAs emit. Full parse coverage lives on `time` crate; we
/// don't need to pull it in for the fallback.
/// Parse an RFC 5322 `Date:` header value to unix epoch seconds (UTC).
///
/// Delegates to `chrono::DateTime::parse_from_rfc2822`, which handles
/// every real-world variant we see: `Sat, 13 Jun 2026 06:01:22 +0000`,
/// `Fri, 3 Jul 2026 02:40:42 +0900` (Gmail), `13 Jun 2026 06:01:22 GMT`
/// (no day-of-week), and named zones (`GMT`/`UTC`/`EST`/…). Timezones
/// are correctly normalised to UTC before the epoch conversion — the
/// previous hand-rolled parser dropped the zone entirely, so an email
/// stamped in JST landed nine hours off and inbound replies could sort
/// ahead of the sent copy.
///
/// Returns `None` when the header is empty / unparseable.
fn parse_rfc5322_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    // Retry ladder for the messy real world:
    //   1. Strip a trailing " (CFWS)" comment (RFC 5322 §3.3 permits it,
    //      chrono rejects it).
    //   2. Strip a leading "Weekday, " prefix — many senders ship a
    //      day-of-week that disagrees with the date (chrono treats that
    //      as Impossible even though the timestamp is well-formed).
    let no_comment = s.split(" (").next().unwrap_or(s).trim_end();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(no_comment) {
        return Some(dt.timestamp());
    }
    let no_dow = match no_comment.find(", ") {
        Some(idx) => no_comment[idx + 2..].trim_start(),
        None => no_comment,
    };
    chrono::DateTime::parse_from_rfc2822(no_dow)
        .ok()
        .map(|dt| dt.timestamp())
}

/// Extract the delivery epoch from a Maildir filename. The Maildir
/// naming convention (`<epoch>.M<micro>P<pid>Q<seq>.<host>`) records
/// the delivery second in the leading component — a reliable fallback
/// when the message's `Date:` header is missing or unparseable. Filter
/// out obviously bogus epochs (<= year 2000) so we don't backdate
/// modern mail into 1970 territory.
fn maildir_filename_epoch(name: &str) -> Option<i64> {
    let first = name.split('.').next()?;
    let n: i64 = first.parse().ok()?;
    if n > 946_684_800 { Some(n) } else { None }
}

/// Whether a maildir filename carries the \Seen flag — the `:2,` info
/// section lists flags alphabetically (`...:2,RS` etc.).
fn maildir_seen_flag(name: &str) -> bool {
    match name.rsplit_once(":2,") {
        Some((_, info)) => info.contains('S'),
        None => false,
    }
}

/// Fall back to the file's mtime as the delivery epoch when both the
/// `Date:` header and the maildir filename yield nothing usable.
fn file_mtime_epoch(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

#[derive(Debug, Clone)]
struct MailFile {
    /// blob_ref stored in the message wire. Either just the maildir
    /// filename (for INBOX/cur+new files) or `<subfolder>/<filename>`
    /// (for files under Maildir++ subfolders like `.Sent/`). The
    /// prefix lets `enrich_with_body` locate the file when it lives
    /// outside INBOX — otherwise `MaildirStore::fetch` returns None
    /// and the UI shows "(no text content)".
    filename: String,
    size: u32,
    message_id: String,
    in_reply_to: String,
    references: Vec<String>,
    subject: String,
    date: i64,
    from: String,
    to: String,
    /// maildir info-section \Seen flag (`...:2,...S...`) — the on-disk
    /// read/unread fact. Self-heal must respect it or every boot
    /// resurrects already-read mail as unread.
    seen: bool,
    /// Sender-auth verdict from the file's `Authentication-Results`
    /// header (`verified` / `suspicious` / `unverified` / `""`).
    sender_trust: String,
}

/// Walk the user's maildir(s) and populate any thread whose messages
/// zset is empty. Best-effort — logs and continues on parse errors.
///
/// Coverage:
/// - INBOX (`cur/`, `new/`) and every top-level Maildir++ subfolder
///   under the user's maildir root (`.Sent/`, `.Drafts/`, `.Trash/`,
///   `.Junk/`, and any custom folder created by IMAP clients). Ensures
///   sent-copy messages get picked up even when they live in `.Sent`.
/// - Threading: for each file, resolve its "conversation root" via
///   References[0] → In-Reply-To → own Message-ID. All files sharing a
///   root get upserted into that root's fastcore thread.
///
/// Returns `true` when this sweep actually repaired something. The
/// caller uses that to back off: a mailbox that is already consistent
/// must not be re-scanned every 30 s forever (2026-07-19 — this loop
/// was re-reading all ~48k maildir headers per cycle on staging).
async fn healed_from_maildir(state: &Arc<FastcoreState>, user: &str, since: i64) -> bool {
    let Some((local, domain)) = user.split_once('@') else {
        return false;
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(&root).join(domain).join(local);
    // Incremental filter. Every path that writes a maildir file now
    // indexes it write-through (spool_drain, bounce, IMAP APPEND/COPY,
    // REST copy/move), so the only gap this sweep still has to close is
    // a process that died between the file landing and the index write
    // — which makes the file necessarily recent. `since = 0` means a
    // full sweep (boot + the daily backstop).
    //
    // The cutoff reads the timestamp out of the maildir filename rather
    // than stat'ing mtime: maildir names are `<epoch>.<unique>.<host>`
    // by spec and are monotonic per delivery, whereas mtime is rewritten
    // by anything that rsyncs or touches the store.
    let recent_enough = |name: &str| -> bool {
        if since == 0 {
            return true;
        }
        // Unparseable name → always inspect it; being wrong here costs
        // one header read, being wrong the other way loses a message.
        maildir_filename_epoch(name).is_none_or(|ts| ts >= since)
    };
    // Collect (subfolder_prefix, path) pairs. `subfolder_prefix` is
    // empty for INBOX and `.<foldername>` for Maildir++ subfolders.
    // It's later prepended to the blob_ref so `enrich_with_body` can
    // locate the file: INBOX files stay bare filenames (matches the
    // pg-dump migration's convention), subfolder files become
    // `.Sent/<filename>`.
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for sub in ["cur", "new"] {
        let dir = base.join(sub);
        if let Ok(iter) = std::fs::read_dir(&dir) {
            for entry in iter.flatten() {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false)
                    && recent_enough(&entry.file_name().to_string_lossy())
                {
                    files.push((String::new(), entry.path()));
                }
            }
        }
    }
    // Maildir++ subfolders (`.Sent`, `.Drafts`, `.Junk`, custom …).
    // IMAP clients that APPEND to `.Sent` write the user's outgoing
    // messages here — without walking these, the Sent tab is stuck at
    // "only threads whose sent-copy landed in INBOX via mirror_send".
    if let Ok(iter) = std::fs::read_dir(&base) {
        for entry in iter.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') {
                continue;
            }
            let sub_base = entry.path();
            for sub in ["cur", "new"] {
                let dir = sub_base.join(sub);
                if let Ok(iter) = std::fs::read_dir(&dir) {
                    for e in iter.flatten() {
                        if e.file_type().map(|t| t.is_file()).unwrap_or(false)
                            && recent_enough(&e.file_name().to_string_lossy())
                        {
                            files.push((name.clone(), e.path()));
                        }
                    }
                }
            }
        }
    }
    if files.is_empty() {
        return false;
    }

    // Parse headers for every file. Only load the first 16 KB.
    let mut parsed: Vec<MailFile> = Vec::with_capacity(files.len());
    for (subfolder, path) in &files {
        let bare = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        // Prepend Maildir++ subfolder when set so `enrich_with_body`
        // can route `MaildirStore::fetch` to the right sub-maildir.
        let blob_ref = if subfolder.is_empty() {
            bare.clone()
        } else {
            format!("{subfolder}/{bare}")
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let size = bytes.len() as u32;
        let head = &bytes[..bytes.len().min(16 * 1024)];
        let (message_id, in_reply_to, references, subject, date, from, to) = extract_headers(head);
        if message_id.is_empty() {
            continue;
        }
        // If the RFC 5322 `Date:` header was missing or unparseable
        // (some mailers ship malformed dates and many self-injected
        // notifications have none), fall back to the maildir delivery
        // epoch encoded in the filename, then to file mtime, then to
        // 0. Without these fallbacks the affected messages sorted to
        // 1970 and inbound replies could end up ahead of the sent
        // copy in the thread timeline.
        let date = if date > 0 {
            date
        } else {
            maildir_filename_epoch(&bare)
                .or_else(|| file_mtime_epoch(path))
                .unwrap_or(0)
        };
        parsed.push(MailFile {
            filename: blob_ref,
            size,
            message_id,
            in_reply_to,
            references,
            subject,
            date,
            from,
            to,
            seen: maildir_seen_flag(&bare),
            sender_trust: extract_sender_trust(&bytes),
        });
    }

    // Bucket by resolved conversation root. v2.9.5: consult the
    // msgid→thread index first (same rule as live ingest) so self-heal
    // groups a reply into the thread its ancestors actually live in;
    // the raw-header guess is only the fallback for unknown chains.
    let mut by_root: std::collections::HashMap<String, Vec<&MailFile>> =
        std::collections::HashMap::new();
    for m in &parsed {
        let root = match resolve_thread_by_ancestry(
            state,
            user,
            &m.message_id,
            &m.in_reply_to,
            &m.references,
            &m.subject,
        ) {
            Some(tid) => tid,
            None => {
                if let Some(first) = m.references.first() {
                    first.clone()
                } else if !m.in_reply_to.is_empty() {
                    m.in_reply_to.clone()
                } else {
                    m.message_id.clone()
                }
            }
        };
        by_root.entry(root).or_default().push(m);
    }

    // UID backfill — one-time per boot per user. Repair any
    // MessageWire that self-heal wrote before we started allocating
    // uids (all showed uid=0, breaking /api/mail/messages/{uid}/…
    // attachment endpoints). Guard on a persistent flag so subsequent
    // ticks don't re-scan the full mailbox. Bump the sentinel key when
    // the migration format changes to force another sweep.
    // v2: bumped after finding deliver_message wrote uid=0 wires for
    // every web-sent mirror copy until 2026-07-03 — one more full sweep
    // repairs the backlog now that the write path allocates correctly.
    let uid_flag_key = format!("mailrs:user:{user}:uid_backfill_v2");
    let need_uid_backfill = state
        .mailbox
        .store_ref()
        .get(uid_flag_key.as_bytes())
        .ok()
        .flatten()
        .is_none();
    if need_uid_backfill {
        // Declared rows; `user_threads_by_activity` is legacy and unwritten,
        // so this healed 168 threads install-wide instead of 30,562.
        let all_tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        let mut uid_healed = 0u32;
        for tid in &all_tids {
            let tid = tid.as_str();
            let msgs = state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default();
            for payload in msgs {
                let Ok(mut wire) = serde_json::from_slice::<
                    mailrs_core_api::method::message::MessageWire,
                >(&payload) else {
                    continue;
                };
                if wire.uid != 0 {
                    continue;
                }
                let uid = state
                    .mailbox
                    .allocate_uid(user, &wire.message_id)
                    .unwrap_or(0);
                if uid == 0 {
                    continue;
                }
                wire.uid = uid;
                if let Ok(new_payload) = serde_json::to_vec(&wire) {
                    let _ = state.mailbox.upsert_message(
                        &wire.thread_id,
                        &wire.message_id,
                        wire.internal_date,
                        &new_payload,
                    );
                }
                uid_healed += 1;
            }
        }
        let _ = state.mailbox.store_ref().set(uid_flag_key.as_bytes(), b"1");
        if uid_healed > 0 {
            tracing::info!(%user, uid_healed, "self-heal: uid backfill (one-shot)");
        }
    }

    // Walk threads and heal — two branches, both idempotent:
    // (a) zset empty → populate all bucket messages (original behaviour)
    // (b) zset non-empty but bucket has message-ids not in it → G14.2
    //     diff branch. Catches the "spool_drain crashed / dropped a file
    //     mid-tick" case: the file's on disk but the wire never got
    //     written, so the message is invisible to the API. Diffing by
    //     message-id closes that gap without touching the fast path.
    // Declared rows. The zset this replaced was legacy and unwritten; the
    // 0..999 slice it took is kept as an explicit bound because this runs
    // on a timer, and the count below reports what was actually walked.
    let mut tids = state
        .mailbox
        .all_thread_ids_for_user(user)
        .unwrap_or_default();
    tids.truncate(1000);
    let mut healed_threads = 0u32;
    let mut healed_msgs = 0u32;
    let mut diff_healed_threads = 0u32;
    let mut diff_healed_msgs = 0u32;
    for tid in &tids {
        let tid = tid.as_str();
        let msg_zset = mailrs_mailbox_kevy::keys::thread_messages(tid);
        let existing_count = state
            .mailbox
            .store_ref()
            .zcard(msg_zset.as_bytes())
            .unwrap_or(0);
        let Some(bucket) = by_root.get(tid) else {
            continue;
        };

        // Compute (message_id → &MailFile) index for the bucket up front —
        // used by both branches. Filter out entries with no Message-ID so
        // upsert_message doesn't key on an empty string (which would
        // conflate distinct files into one wire).
        let bucket_by_mid: std::collections::HashMap<&str, &&MailFile> = bucket
            .iter()
            .filter(|m| !m.message_id.is_empty())
            .map(|m| (m.message_id.as_str(), m))
            .collect();
        if bucket_by_mid.is_empty() {
            continue;
        }

        // Determine which of the bucket's messages need writing:
        // empty zset → all of them; non-empty → diff against existing
        // wire payloads' message_id field.
        let missing_mids: Vec<&str> = if existing_count == 0 {
            bucket_by_mid.keys().copied().collect()
        } else {
            let existing_mids: std::collections::HashSet<String> = state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|payload| {
                    serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(
                        &payload,
                    )
                    .ok()
                })
                .map(|w| w.message_id)
                .collect();
            bucket_by_mid
                .keys()
                .copied()
                .filter(|mid| !existing_mids.contains(*mid))
                .collect()
        };
        if missing_mids.is_empty() {
            continue;
        }
        // Sort by date so zadd scores are chronological — matches the
        // spool_drain write path so mixed populate/diff runs produce the
        // same ordering.
        let mut to_write: Vec<&MailFile> = missing_mids
            .into_iter()
            .filter_map(|mid| bucket_by_mid.get(mid).copied().copied())
            .collect();
        to_write.sort_by_key(|m| m.date);
        for m in &to_write {
            // allocate_uid is idempotent — reruns return the previously-
            // issued uid via the uid_by_mid reverse index, so it's safe
            // to run either branch multiple times.
            let uid = state.mailbox.allocate_uid(user, &m.message_id).unwrap_or(0);
            let wire = mailrs_core_api::method::message::MessageWire {
                id: 0,
                mailbox_id: 0,
                uid,
                blob_ref: m.filename.clone(),
                sender: m.from.clone(),
                recipients: m.to.clone(),
                subject: m.subject.clone(),
                date: m.date,
                internal_date: m.date,
                size: m.size,
                flags: 1,
                message_id: m.message_id.clone(),
                in_reply_to: m.in_reply_to.clone(),
                sender_trust: m.sender_trust.clone(),
                thread_id: tid.to_string(),
                modseq: 0,
                user_address: user.to_string(),
            };
            let payload = match serde_json::to_vec(&wire) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let _ = state
                .mailbox
                .upsert_message(tid, &wire.message_id, m.date, &payload);
            let _ = state
                .mailbox
                .set_thread_for_message_id(user, &wire.message_id, tid);
            if existing_count == 0 {
                healed_msgs += 1;
            } else {
                diff_healed_msgs += 1;
            }
        }
        if existing_count == 0 {
            healed_threads += 1;
        } else {
            diff_healed_threads += 1;
        }
    }
    if healed_threads > 0 {
        tracing::info!(
            %user, healed_threads, healed_msgs, files_scanned = parsed.len(),
            "self-heal (maildir): populated missing messages"
        );
    }
    if diff_healed_threads > 0 {
        // G14.2 diff branch fired — surface it separately so an oncall
        // can distinguish "brand-new thread stitched from scratch" from
        // "existing thread patched with a message the drain missed".
        tracing::info!(
            %user,
            diff_healed_threads,
            diff_healed_msgs,
            files_scanned = parsed.len(),
            "self-heal (maildir): diff branch patched missed messages (G14.2)"
        );
    }

    // ── Sent-index backfill ──────────────────────────────────────
    //
    // The historical migration derived senders_csv from monolith's
    // sent_count aggregate, which false-negatives on many threads the
    // user actually sent messages to (only 22 of ~200 sent messages
    // were picked up on lihao@golia.jp). Walk every thread bucket and,
    // if any of its maildir files has From: == user, add the thread's
    // fastcore tid to `mailrs:user:<u>:threads:sent` scored by the
    // latest sent message's date. Idempotent: zadd overwrites the
    // score for a tid that's already there.
    let sent_key = mailrs_mailbox_kevy::keys::user_threads_sent(user);
    let mut sent_added = 0u32;
    let mut created = 0u32;
    for (root, bucket) in &by_root {
        let sent_here: Vec<&&MailFile> = bucket
            .iter()
            .filter(|m| mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user))
            .collect();
        let is_sender_thread = !sent_here.is_empty();
        let thread_key = mailrs_mailbox_kevy::keys::thread(root);
        let exists = state
            .mailbox
            .store_ref()
            .hexists(thread_key.as_bytes(), b"count")
            .unwrap_or(false);
        if !exists {
            // Create a minimal thread aggregate from scratch — inbound
            // OR outbound. Skipping non-sender threads here was the
            // reason fresh Gmail arrivals (files present in maildir but
            // no matching kevy hash) never showed up in the inbox: the
            // "heal missing messages" branch above only touches threads
            // already in the by_activity zset, so a genuinely new
            // arrival had no path in. Create here for every bucket.
            let mut ordered: Vec<&MailFile> = bucket.to_vec();
            ordered.sort_by_key(|m| m.date);
            for m in &ordered {
                let category = "inbox";
                let is_own = mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user);
                let unread = !m.seen && !is_own;
                let arrival = mailrs_mailbox_kevy::MessageArrival {
                    thread_id: root,
                    user,
                    subject: &m.subject,
                    senders_csv: &m.from,
                    latest_date: m.date,
                    latest_preview: "",
                    category,
                    unread,
                    is_own,
                };
                let _ = state.mailbox.record_message_arrival(&arrival);
                // Side sink: contacts autocomplete.
                crate::live_sync::upsert_contacts(user, &m.from);
                // Also write the message blob for enrich_with_body.
                let uid = state.mailbox.allocate_uid(user, &m.message_id).unwrap_or(0);
                let wire = mailrs_core_api::method::message::MessageWire {
                    id: 0,
                    mailbox_id: 0,
                    uid,
                    blob_ref: m.filename.clone(),
                    sender: m.from.clone(),
                    recipients: m.to.clone(),
                    subject: m.subject.clone(),
                    date: m.date,
                    internal_date: m.date,
                    size: m.size,
                    flags: 1,
                    message_id: m.message_id.clone(),
                    in_reply_to: m.in_reply_to.clone(),
                    sender_trust: m.sender_trust.clone(),
                    thread_id: root.clone(),
                    modseq: 0,
                    user_address: user.to_string(),
                };
                if let Ok(payload) = serde_json::to_vec(&wire) {
                    let _ = state
                        .mailbox
                        .upsert_message(root, &m.message_id, m.date, &payload);
                }
                let _ = state
                    .mailbox
                    .set_thread_for_message_id(user, &m.message_id, root);
            }
            created += 1;
        }
        if !is_sender_thread {
            // Inbound-only thread — created (or already existed), but
            // the user isn't a sender so it doesn't belong in the sent
            // zset. Skip the sent-index maintenance below.
            continue;
        }
        // Score the sent zset by the aggregate's own latest_date —
        // that's what the UI displays as the row's date pill, so this
        // guarantees the list is sorted identically to what the pill
        // shows. Prior versions scored by our own local bucket-max,
        // which drifted from the aggregate whenever record_message_arrival
        // was called separately (e.g. via mirror_send during a live
        // send) and left the two in disagreement → apparent random order.
        //
        // If the stored aggregate latest_date is stale/zero (e.g. the
        // hash was created back when parse_rfc5322_date was broken and
        // fed 0), prefer the bucket's true max date and heal the hash
        // + by_activity index so the row stops sinking to the bottom.
        // v2 Stage B.1: 6 sequential RMW ops (hget latest_date / hset
        // latest_date / zadd by_activity / zadd sent_key / hget
        // senders_csv / hset senders_csv) collapsed into a single
        // AtomicCtx closure. Two concurrent self-heal or ingest calls
        // on the same thread now serialize on the shard write lock —
        // no interleaving read-then-stale-write against the aggregate.
        // Display-date semantics (2026-07-18): the row follows the last
        // INBOUND message, so the "stale hash" heal must not treat the
        // user's own sent copy as newer truth — that exact write undid
        // the backfill repair every 30 s. Sent-only threads keep the
        // plain max.
        let bucket_max = bucket
            .iter()
            .filter(|m| !mailrs_mailbox_kevy::senders_csv_contains_user(&m.from, user))
            .map(|m| m.date)
            .max()
            .unwrap_or_else(|| bucket.iter().map(|m| m.date).max().unwrap_or(0));
        let by_activity = mailrs_mailbox_kevy::keys::user_threads_by_activity(user);
        // Every write below is conditional, and the closure reports
        // whether it actually changed anything. A self-heal that
        // re-does its work every cycle is a busy-wait, not a heal: this
        // loop used to zadd the sent zset and bump `sent_added`
        // unconditionally, so a fully-healed mailbox still logged
        // `sent_added=255 created=0` every 31 s forever (2026-07-19).
        let changed = state
            .mailbox
            .store_ref()
            .atomic(|ctx| {
                let mut changed = false;
                let stored_latest = ctx
                    .hget(thread_key.as_bytes(), b"latest_date")?
                    .and_then(|v| String::from_utf8(v).ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);
                let agg_latest = std::cmp::max(stored_latest, bucket_max);
                if agg_latest > stored_latest {
                    ctx.hset(
                        thread_key.as_bytes(),
                        &[(b"latest_date" as &[u8], agg_latest.to_string().as_bytes())],
                    )?;
                    ctx.zadd(
                        by_activity.as_bytes(),
                        &[(agg_latest as f64, root.as_bytes())],
                    )?;
                    changed = true;
                }
                let want = agg_latest as f64;
                if ctx.zscore(sent_key.as_bytes(), root.as_bytes())? != Some(want) {
                    ctx.zadd(sent_key.as_bytes(), &[(want, root.as_bytes())])?;
                    changed = true;
                }
                // Merge user into the thread's senders_csv so future
                // upsert_thread invocations (mark_read etc.) don't drop
                // sent-index membership.
                let cur_csv = ctx
                    .hget(thread_key.as_bytes(), b"senders_csv")?
                    .and_then(|v| String::from_utf8(v).ok())
                    .unwrap_or_default();
                if !mailrs_mailbox_kevy::senders_csv_contains_user(&cur_csv, user) {
                    let new_csv = if cur_csv.is_empty() {
                        user.to_string()
                    } else {
                        format!("{cur_csv}, {user}")
                    };
                    ctx.hset(
                        thread_key.as_bytes(),
                        &[(b"senders_csv" as &[u8], new_csv.as_bytes())],
                    )?;
                    changed = true;
                }
                Ok(changed)
            })
            .unwrap_or(false);
        if changed {
            sent_added += 1;
        }
    }
    if sent_added > 0 || created > 0 {
        tracing::info!(
            %user, sent_added, created,
            "self-heal (maildir): sent-index backfill"
        );
    }
    healed_threads > 0 || diff_healed_threads > 0 || sent_added > 0 || created > 0
}

/// Extract the searchable text of a message: the `text/plain` part if
/// there is one, else the `text/html` part flattened. Returns `None`
/// when neither exists (a bare attachment, say) so the caller can skip
/// writing an empty row.
pub(crate) fn body_text_for_search(raw: &[u8]) -> Option<String> {
    let root = mailrs_mime::parse(raw);
    let mut html: Option<String> = None;
    for part in root.walk() {
        match part.content_type.mime_type().as_str() {
            "text/plain" => {
                if let Some(t) = part.body_text() {
                    return Some(t);
                }
            }
            "text/html" if html.is_none() => html = part.body_text(),
            _ => {}
        }
    }
    if let Some(h) = html {
        return Some(html2text::from_read(h.as_bytes(), 100).unwrap_or(h));
    }
    root.body_text()
}

/// Write-through ingest for a file the spool drain just delivered to
/// maildir: thread aggregate + message wire + uid + side sinks, all at
/// delivery time.
///
/// Before this existed the drain wrote ONLY maildir and relied on the
/// periodic self-heal to surface the message — but self-heal handles
/// just two shapes (thread hash missing / messages zset empty), so a
/// reply landing in an EXISTING thread never became visible (G14).
/// Self-heal remains the crash-recovery backstop; this is the primary
/// path.
pub(crate) fn ingest_delivered_file(
    state: &Arc<FastcoreState>,
    addr: &str,
    blob_ref: &str,
    body: &[u8],
    target_folder: &str,
) {
    let head = &body[..body.len().min(16 * 1024)];
    let (message_id, in_reply_to, references, subject, date, from, to) = extract_headers(head);
    if message_id.is_empty() {
        // no Message-ID header — leave it to self-heal's filename-based
        // fallbacks rather than fabricating an id here
        return;
    }
    let bare = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    let date = if date > 0 {
        date
    } else {
        maildir_filename_epoch(bare).unwrap_or(0)
    };
    // v2.9.5 threading fix — prefer the thread an ancestor actually
    // landed in (msgid index) over deriving one from raw headers.
    // References[0] is NOT a stable conversation root (each hop can
    // rewrite it), which is how conversations fragmented.
    let root = match resolve_thread_by_ancestry(
        state,
        addr,
        &message_id,
        &in_reply_to,
        &references,
        &subject,
    ) {
        Some(tid) => tid,
        None => {
            if let Some(first) = references.first() {
                first.clone()
            } else if !in_reply_to.is_empty() {
                in_reply_to.clone()
            } else {
                message_id.clone()
            }
        }
    };
    let is_own = mailrs_mailbox_kevy::senders_csv_contains_user(&from, addr);
    let unread = !is_own;
    // v2.4.0 Phase 2 (RFC-A) — plumb the SMTP-level target_folder
    // decision (from `crates/receiver/src/smtp_session/events/data/antispam.rs`
    // where DeliveryDecision::Junk yields target_folder="Junk") into the
    // per-thread category. mailbox-kevy's `upsert_thread` reads
    // `category ∈ {"spam", "scam"}` as the Junk-zset trigger, so
    // stamping "spam" here makes the antispam verdict actually route
    // to the Junk folder on the read side. Any sieve fileinto target
    // that maps to "Junk" is treated the same. Everything else
    // (INBOX / custom sieve folders) keeps category="inbox".
    // v2.9 triage — non-junk mail is further sorted into
    // inbox/notification/promotion by the multi-class Bayes classifier
    // (`bucket_of` then routes it to the matching folder zset).
    // Cold-start / low-confidence → "inbox".
    let category = if target_folder.eq_ignore_ascii_case("junk") {
        "spam"
    } else {
        crate::bayes_train::classify_triage(state, body).unwrap_or("inbox")
    };
    let arrival = mailrs_mailbox_kevy::MessageArrival {
        thread_id: &root,
        user: addr,
        subject: &subject,
        senders_csv: &from,
        latest_date: date,
        latest_preview: "",
        category,
        unread,
        is_own,
    };
    if let Err(e) = state.mailbox.record_message_arrival(&arrival) {
        tracing::warn!(error = %e, %addr, %root, "drain ingest: record_message_arrival failed");
    }
    // Importance follows the latest INBOUND message, like the thread's
    // display fields — the user's own reply must not restate it.
    if !is_own {
        crate::importance::score_inbound(state, addr, &root, &from, head, body);
    }
    crate::live_sync::upsert_contacts(addr, &from);
    crate::live_sync::adjust_usage_bytes(addr, body.len() as i64);
    let m = crate::imap::backend::bump_modseq(state, addr);
    crate::imap::backend::set_file_modseq(state, addr, bare, m);
    let _ = state.notify.send(addr.to_string());
    crate::live_sync::publish_new_mail(addr, &root, &from, &subject, "");
    let uid = state.mailbox.allocate_uid(addr, &message_id).unwrap_or(0);
    let wire = mailrs_core_api::method::message::MessageWire {
        id: 0,
        mailbox_id: 0,
        uid,
        blob_ref: blob_ref.to_string(),
        sender: from,
        recipients: to,
        subject,
        date,
        internal_date: date,
        size: body.len() as u32,
        flags: if unread { 0 } else { 1 },
        message_id: message_id.clone(),
        in_reply_to,
        sender_trust: extract_sender_trust(body),
        thread_id: root.clone(),
        modseq: 0,
        user_address: addr.to_string(),
    };
    match serde_json::to_vec(&wire) {
        Ok(payload) => {
            if let Err(e) = state
                .mailbox
                .upsert_message(&root, &message_id, date, &payload)
            {
                tracing::warn!(error = %e, %addr, %root, "drain ingest: upsert_message failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "drain ingest: wire serialize failed"),
    }
    // register this message's id → thread so future replies that cite it
    // (In-Reply-To / References) resolve into the same conversation.
    let _ = state
        .mailbox
        .set_thread_for_message_id(addr, &message_id, &root);
    // Index the body for full-text search. Costs one MIME parse on a
    // path that already has the bytes in hand, and it is what makes
    // search cover message contents rather than just headers.
    if let Some(text) = body_text_for_search(body)
        && let Err(e) = state.mailbox.index_message_text(&message_id, &root, &text)
    {
        tracing::warn!(error = %e, %addr, %message_id, "index_message_text failed");
    }
}

pub fn build_router(state: Arc<FastcoreState>) -> Router {
    let base = base_router(state.clone());
    // One Router for all business routes so matchit's trie sees the
    // full set at once. Earlier split into convo + thread Routers
    // hit a route-resolution bug where only the first-registered
    // route under /v1/users/{user}/conversations matched at runtime —
    // probable matchit collision between `conversations:list` (literal
    // ":list") and `conversations/categories` (path-separator). A
    // single Router with all routes registered side-by-side resolves it.
    let business =
        Router::new()
            .route(conv::PATH_LIST_CONVERSATIONS, post(list_conversations))
            .route(conv::PATH_SEARCH_CONVERSATIONS, post(search_conversations))
            .route(
                conv::PATH_CONVERSATIONS_BY_THREAD_IDS,
                post(conversations_by_thread_ids),
            )
            .route(conv::PATH_CONVERSATION_CATEGORIES, get(get_categories))
            .route(conv::PATH_UNSEEN_COUNT, get(get_unseen_count))
            .route(th::PATH_LIST_THREAD_MESSAGES, get(thread_messages))
            .route(th::PATH_LIST_SENT_MESSAGES, get(list_sent_messages))
            .route(
                th::PATH_FIND_THREAD_BY_MESSAGE_ID,
                get(find_thread_by_message_id),
            )
            .route(th::PATH_BACKFILL_THREADING, post(backfill_threading_route))
            .route(
                "/v1/admin/backfill-decode-headers",
                post(backfill_decode::backfill_decode_headers_route),
            )
            .route("/v1/admin/threads:split-message", post(split_message_route))
            .route("/v1/admin/maintenance:rewrite-aof", post(rewrite_aof_route))
            .route(th::PATH_DELIVER_MESSAGE, post(deliver_message))
            .route(th::PATH_MARK_READ, post(mark_read))
            .route(th::PATH_MARK_ALL_READ, post(mark_all_read_route))
            .route(th::PATH_MARK_UNREAD, post(mark_unread_route))
            .route(th::PATH_SNOOZE, put(snooze_thread_route))
            .route(th::PATH_UNSNOOZE, delete(unsnooze_thread_route))
            .route(th::PATH_PIN, post(pin_thread))
            .route(th::PATH_UNPIN, post(unpin_thread))
            .route(th::PATH_STAR, post(star_thread))
            .route(th::PATH_UNSTAR, post(unstar_thread))
            .route(th::PATH_ARCHIVE, post(archive_thread))
            .route(th::PATH_UNARCHIVE, post(unarchive_thread))
            .route(th::PATH_MARK_JUNK, post(mark_junk))
            .route(th::PATH_MARK_NOT_JUNK, post(mark_not_junk))
            .route(th::PATH_MARK_NOTIFICATION, post(mark_notification))
            .route(th::PATH_MARK_PROMOTION, post(mark_promotion))
            .route(th::PATH_MOVE_TO_INBOX, post(move_to_inbox))
            .route(th::PATH_DELETE_THREAD, delete(delete_thread))
            .route(adm::PATH_GET_ACCOUNT_HASH, get(get_account_with_hash))
            .route(adm::PATH_EFFECTIVE_PERMISSIONS, get(effective_permissions))
            .route(
                adm::PATH_LIST_ACCOUNTS,
                get(list_accounts).post(add_account_route),
            )
            .route(
                adm::PATH_UPDATE_ACCOUNT,
                put(update_account_route).delete(remove_account_route),
            )
            .route(adm::PATH_SET_QUOTA, post(set_quota_route))
            .route(
                adm::PATH_UPDATE_RECOVERY_EMAIL,
                post(set_recovery_email_route),
            )
            .route(adm::PATH_SET_ACCOUNT_PASSWORD, post(set_password_route))
            .route(adm::PATH_SET_MESSAGE_FLAGS, post(set_message_flags_route))
            // Aliases live in the fastcore-embedded kevy so the spool drain
            // (also in-process) can resolve `contact@golia.jp -> lihao` and
            // similar single-hop forwards. Distinct namespace from webapi's
            // network-kevy `admin:aliases` hash — that older store is not
            // consulted by the drain and stays around only until UI wiring
            // catches up.
            .route(
                "/v1/admin/aliases:local",
                get(list_local_aliases).post(upsert_local_alias),
            )
            .route(
                "/v1/admin/aliases:local/{source}",
                delete(delete_local_alias_route),
            )
            // Ops endpoint — reset every user's ingest cursor to 0 so the
            // next sync tick re-processes historic threads and (via the
            // Group F diff path) backfills messages fastcore missed under
            // the older "skip-existing" ingest behaviour.
            .route(
                "/v1/admin/sync/reset-cursors",
                post(reset_sync_cursors_route),
            )
            // Ops endpoint — one-shot pre-P6 legacy keyspace sweep
            // (Phase 11.2 embedded half). In-process so no AOF
            // double-open OOM; idempotent.
            .route(
                "/v1/admin/maintenance:sweep-legacy-admin-keys",
                post(sweep_legacy_admin_keys_route),
            )
            // Ops endpoint — seed the Bayesian corpus from existing
            // junk (spam) + inbox (ham) folders. One-shot; refuses if
            // the corpus is already non-empty.
            .route(
                "/v1/admin/maintenance:bayes-bootstrap",
                post(bayes_bootstrap_route),
            )
            // Ops endpoint — seed the v2.9 multi-class triage corpus +
            // re-sort existing Inbox mail into N/P (idempotent).
            .route(
                "/v1/admin/maintenance:backfill-triage",
                post(backfill_triage_route),
            )
            // Segmented promotion of existing threads into the
            // `threaduser` table's membership rows (v4 TABLE migration).
            // Paged on purpose: a full scan competes with live traffic
            // for the same store, so the caller drives it in batches.
            .route(
                "/v1/admin/maintenance:backfill-thread-user",
                post(backfill_thread_user_route),
            )
            // Rebuild thread counters from the messages they summarise.
            // The arrival path increments them by hand next to an index
            // that dedupes, so a message delivered to two local
            // mailboxes counts twice.
            .route(
                "/v1/admin/maintenance:recount-threads",
                post(recount_threads_route),
            )
            // The same two copies, compared rather than repaired. The
            // gate for reading from the per-user one.
            .route(
                "/v1/admin/maintenance:shadow-counts",
                post(shadow_counts_route),
            )
            .route(
                "/v1/admin/maintenance:sent-axis-shadow",
                post(sent_axis_shadow_route),
            )
            .route(
                "/v1/admin/maintenance:legacy-zset-census",
                post(legacy_zset_census_route),
            )
            // Engine-side reconciliation for the declared table: drift
            // recheck per compiled index plus a column-type spot check.
            .route(
                "/v1/admin/maintenance:table-verify",
                post(table_verify_route),
            )
            // Row-level census behind the VERIFY counters — answers
            // "which rows are missing from an index", which VERIFY
            // reports as a count and not an identity.
            .route(
                "/v1/admin/maintenance:threaduser-census",
                post(threaduser_census_route),
            )
            // Deletes the legacy per-user thread zsets. Nothing writes
            // or reads them any more; this reclaims the memory.
            .route(
                "/v1/admin/maintenance:drop-legacy-zsets",
                post(drop_legacy_zsets_route),
            )
            // RAM versus disk, so tiering can be judged on numbers.
            .route("/v1/admin/maintenance:tier-info", post(tier_info_route))
            // Shadow read — the engine's answer against the
            // hand-maintained zset's, before any read is cut over.
            .route("/v1/admin/maintenance:shadow-read", post(shadow_read_route))
            // Contact relationship counters, rebuilt from message
            // history so importance scoring sees existing correspondents
            // instead of waiting months for new traffic (idempotent).
            .route(
                "/v1/admin/maintenance:backfill-contact-relationships",
                post(backfill_contact_relationships_route),
            )
            // Importance verdicts for threads that predate the feature —
            // scoring only runs at ingest, so without this every existing
            // thread would stay blank forever.
            .route(
                "/v1/admin/maintenance:backfill-thread-importance",
                post(backfill_thread_importance_route),
            )
            .route(mb::PATH_LIST_MAILBOXES, get(list_mailboxes))
            .route(
                msg::PATH_GET_MESSAGE_BY_UID_USER,
                get(get_message_by_uid_for_user),
            )
            // ── shared side-state (network kevy): drafts / signatures /
            // templates — same keys webapi + pg-core read (v2 point 3) ──
            .route(
                adm::PATH_LIST_DRAFTS,
                get(mailrs_core_sidestate::families::prefs::list_drafts::<FastcoreState>)
                    .post(mailrs_core_sidestate::families::prefs::save_draft::<FastcoreState>),
            )
            .route(
                adm::PATH_DELETE_DRAFT,
                delete(mailrs_core_sidestate::families::prefs::delete_draft::<FastcoreState>),
            )
            .route(
                adm::PATH_LIST_SIGNATURES,
                get(mailrs_core_sidestate::families::prefs::list_signatures::<FastcoreState>)
                    .post(mailrs_core_sidestate::families::prefs::save_signature::<FastcoreState>),
            )
            .route(
                adm::PATH_DELETE_SIGNATURE,
                delete(mailrs_core_sidestate::families::prefs::delete_signature::<FastcoreState>),
            )
            .route(
                adm::PATH_LIST_TEMPLATES,
                get(mailrs_core_sidestate::families::prefs::list_templates::<FastcoreState>)
                    .post(mailrs_core_sidestate::families::prefs::save_template::<FastcoreState>),
            )
            .route(
                adm::PATH_DELETE_TEMPLATE,
                delete(mailrs_core_sidestate::families::prefs::delete_template::<FastcoreState>),
            )
            // reactions / webhooks / audit (network kevy)
            .route(
                adm::PATH_GET_THREAD_REACTIONS,
                get(
                    mailrs_core_sidestate::families::admin_state::get_thread_reactions::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_TOGGLE_REACTION,
                put(mailrs_core_sidestate::families::admin_state::toggle_reaction::<FastcoreState>),
            )
            .route(
                adm::PATH_CREATE_WEBHOOK,
                post(mailrs_core_sidestate::families::admin_state::create_webhook::<FastcoreState>),
            )
            .route(
                adm::PATH_LIST_WEBHOOKS,
                get(mailrs_core_sidestate::families::admin_state::list_webhooks::<FastcoreState>),
            )
            .route(
                adm::PATH_DELETE_WEBHOOK,
                delete(
                    mailrs_core_sidestate::families::admin_state::delete_webhook::<FastcoreState>,
                ),
            )
            .route(
                adm::PATH_LIST_AUDIT_LOG,
                get(mailrs_core_sidestate::families::admin_state::list_audit_log::<FastcoreState>)
                    .post(mailrs_core_sidestate::families::admin_state::log_audit::<FastcoreState>),
            )
            // account / alias / domain — switchable mail store (embedded kevy)
            .route(adm::PATH_GET_ACCOUNT, get(routes::mail_admin::get_account))
            .route(
                adm::PATH_LIST_ALIASES,
                get(routes::mail_admin::list_aliases).post(routes::mail_admin::add_alias),
            )
            .route(
                adm::PATH_REMOVE_ALIAS,
                delete(routes::mail_admin::remove_alias),
            )
            .route(
                adm::PATH_LIST_DOMAINS,
                get(routes::mail_admin::list_domains).post(routes::mail_admin::add_domain),
            )
            .route(
                adm::PATH_REMOVE_DOMAIN,
                delete(routes::mail_admin::remove_domain),
            )
            // contacts — shared derived side-state (network kevy)
            .route(
                ct::PATH_SEARCH_CONTACTS,
                get(mailrs_core_sidestate::families::contacts::search_contacts::<FastcoreState>),
            )
            .route(
                ct::PATH_UPSERT_INBOUND,
                post(mailrs_core_sidestate::families::contacts::upsert_inbound::<FastcoreState>),
            )
            .route(
                ct::PATH_CONTACT_SCORING,
                get(mailrs_core_sidestate::families::contacts::contact_scoring::<FastcoreState>),
            )
            .route(
                ct::PATH_HAS_SENT_TO,
                get(mailrs_core_sidestate::families::contacts::has_sent_to::<FastcoreState>),
            )
            .route(
                ct::PATH_SENDER_FEEDBACK,
                post(mailrs_core_sidestate::families::contacts::sender_feedback::<FastcoreState>),
            )
            // analysis — shared derived side-state (network kevy); semantic 501
            .route(
                an::PATH_GET_ANALYSIS,
                get(mailrs_core_sidestate::families::analysis::get_analysis::<FastcoreState>),
            )
            .route(
                an::PATH_COUNT_UNANALYZED,
                get(mailrs_core_sidestate::families::analysis::count_unanalyzed::<FastcoreState>),
            )
            .route(
                an::PATH_BOOST_IMPORTANCE,
                post(mailrs_core_sidestate::families::analysis::boost_importance::<FastcoreState>),
            )
            .route(
                an::PATH_ATTACHMENT_TEXTS,
                get(mailrs_core_sidestate::families::analysis::attachment_texts::<FastcoreState>),
            )
            .route(
                an::PATH_SEMANTIC_SEARCH,
                post(mailrs_core_sidestate::families::analysis::semantic_search),
            )
            // outbound queue — shared network kevy (same keys the sender drains)
            .route(
                ob::PATH_ENQUEUE,
                post(mailrs_core_sidestate::families::outbound::enqueue::<FastcoreState>),
            )
            .route(
                ob::PATH_CLAIM,
                post(mailrs_core_sidestate::families::outbound::claim::<FastcoreState>),
            )
            .route(
                ob::PATH_STATS,
                get(mailrs_core_sidestate::families::outbound::stats::<FastcoreState>),
            )
            .route(
                ob::PATH_RECOVER_STALE,
                post(mailrs_core_sidestate::families::outbound::recover_stale::<FastcoreState>),
            )
            .route(
                ob::PATH_MARK_DELIVERED,
                post(mailrs_core_sidestate::families::outbound::mark_delivered::<FastcoreState>),
            )
            .route(
                ob::PATH_MARK_FAILED,
                post(mailrs_core_sidestate::families::outbound::mark_failed::<FastcoreState>),
            )
            .route(
                ob::PATH_MARK_BOUNCED,
                post(mailrs_core_sidestate::families::outbound::mark_bounced::<FastcoreState>),
            )
            // groups / permissions / api-keys / sieve (network kevy)
            .route(
                adm::PATH_LIST_GROUPS,
                get(mailrs_core_sidestate::families::groups_admin::list_groups::<FastcoreState>),
            )
            .route(
                adm::PATH_GET_GROUP_PERMISSIONS,
                get(
                    mailrs_core_sidestate::families::groups_admin::get_group_permissions::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_LIST_GROUP_MEMBERS,
                get(
                    mailrs_core_sidestate::families::groups_admin::list_group_members::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_GET_ACCOUNT_GROUPS,
                get(
                    mailrs_core_sidestate::families::groups_admin::get_account_groups::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_REMOVE_ACCOUNT_FROM_GROUP,
                delete(
                    mailrs_core_sidestate::families::groups_admin::remove_account_from_group::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_GET_API_KEY_BY_PREFIX,
                get(
                    mailrs_core_sidestate::families::groups_admin::get_api_key_by_prefix::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_TOUCH_API_KEY,
                post(mailrs_core_sidestate::families::groups_admin::touch_api_key::<FastcoreState>),
            )
            .route(
                adm::PATH_GET_SIEVE,
                get(mailrs_core_sidestate::families::groups_admin::get_sieve::<FastcoreState>),
            )
            // mailbox CRUD — reuse the maildir IMAP backend
            .route(mb::PATH_GET_MAILBOX, get(routes::mailbox::get_mailbox))
            .route(
                mb::PATH_GET_MAILBOX_BY_ID,
                get(routes::mailbox::get_mailbox_by_id),
            )
            .route(
                mb::PATH_CREATE_MAILBOX,
                post(routes::mailbox::create_mailbox),
            )
            .route(
                mb::PATH_DELETE_MAILBOX,
                delete(routes::mailbox::delete_mailbox),
            )
            .route(
                mb::PATH_RENAME_MAILBOX,
                post(routes::mailbox::rename_mailbox),
            )
            .route(
                mb::PATH_MAILBOX_STATUS,
                get(routes::mailbox::mailbox_status),
            )
            // message ops — thread-store reads/flags + maildir copy/move/expunge
            .route(
                msg::PATH_GET_MESSAGE_BY_UID,
                get(routes::message::get_message_by_uid),
            )
            .route(
                msg::PATH_FIND_BY_MESSAGE_ID,
                get(routes::message::find_by_message_id),
            )
            .route(msg::PATH_LIST_MESSAGES, get(routes::message::list_messages))
            .route(msg::PATH_CHANGED_SINCE, get(routes::message::changed_since))
            .route(msg::PATH_SET_FLAGS, put(routes::message::set_flags))
            .route(
                msg::PATH_FLAGS_IF_UNCHANGED,
                post(routes::message::flags_if_unchanged),
            )
            .route(msg::PATH_COPY_MESSAGE, post(routes::message::copy_message))
            .route(msg::PATH_MOVE_MESSAGE, post(routes::message::move_message))
            .route(msg::PATH_EXPUNGE, post(routes::message::expunge))
            .with_state(state);

    base.merge(business)
}

fn row_to_wire(r: ThreadRow) -> ConversationSummaryWire {
    ConversationSummaryWire {
        thread_id: r.thread_id,
        subject: r.subject,
        participants: r.senders_csv,
        message_count: r.count.max(0) as u32,
        unread_count: r.unread_count.max(0) as u32,
        last_date: r.latest_date,
        category: r.category,
        flagged: r.starred,
        snippet: r.latest_preview,
        pinned: r.pinned,
        archived: r.archived,
        importance_level: r.importance_level,
        importance_score: r.importance_score as f32,
        requires_action: r.requires_action,
        sent_count: r.sent_count.max(0) as u32,
    }
}

/// `POST /v1/users/{user}/conversations:list`.
async fn list_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Json<conv::ListConversationsResponse> {
    let f = &req.filter;
    // Archived is a tab, not a predicate inside the current folder —
    // the UI's own tab resolver returns 'archived' ahead of the folder
    // it was opened from. The client keeps sending that folder, so
    // honouring both answers "archived within Inbox", which is 0 for a
    // thread filed under notifications and is not what the tab means.
    let folder = if f.archived {
        None
    } else {
        f.folder.as_deref()
    };
    let filter = ListThreadsFilter {
        category: f.category.as_deref(),
        folder,
        pinned: false,
        archived: f.archived,
        has_unread: f.unread.unwrap_or(false),
        has_action: false,
        starred: f.starred.unwrap_or(false),
        before_ts: f.before_ts,
    };
    let limit = if f.limit == 0 { 50 } else { f.limit as usize };
    let (rows, _total) = state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, limit)
        .unwrap_or_else(|_| (Vec::new(), 0));

    let items = rows.into_iter().map(row_to_wire).collect();
    Json(conv::ListConversationsResponse { items })
}

/// `POST /v1/users/{user}/conversations:search` — ranked full-text
/// lookup over the caller's threads.
///
/// Served by the kevy text index declared in `ensure_admin_indexes`,
/// which kevy maintains from its commit hook — the index cannot lag the
/// rows, unlike the external search service this replaced.
async fn search_conversations(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::SearchConversationsRequest>,
) -> Json<conv::SearchConversationsResponse> {
    let limit = if req.limit == 0 {
        20
    } else {
        req.limit as usize
    };
    let hits = state
        .mailbox
        .search_threads(&user, &req.query, limit)
        .unwrap_or_default();
    // Header/subject matches rank first — that is what a user usually
    // means by "find that thread". Body hits fill the remainder, so a
    // phrase that appears only inside a message is still findable.
    let mut tids: Vec<String> = hits.into_iter().map(|(tid, _)| tid).collect();
    if tids.len() < limit
        && let Ok(body_hits) = state
            .mailbox
            .search_message_bodies(&user, &req.query, limit)
    {
        for tid in body_hits {
            if tids.len() >= limit {
                break;
            }
            if !tids.contains(&tid) {
                tids.push(tid);
            }
        }
    }
    let items = tids
        .into_iter()
        .filter_map(|tid| state.mailbox.get_thread(&tid).ok().flatten())
        .filter(|row| match &req.category {
            Some(c) => &row.category == c,
            None => true,
        })
        .map(row_to_wire)
        .collect();
    Json(conv::SearchConversationsResponse { items })
}

/// `GET /v1/users/{user}/conversations/categories` — histogram of
/// category → distinct thread_id count, read straight off the per-
/// category zsets.
async fn get_categories(
    State(state): State<Arc<FastcoreState>>,
    Path(_user): Path<String>,
) -> Json<conv::ConversationCategoriesResponse> {
    // Expanded set — covers monolith's known category vocabulary.
    // Any per-category zset that ZCARD > 0 is returned. UI tabs only
    // render the categories that exist so overshooting is safe.
    //
    // `spam` / `scam` deliberately absent (user directive 2026-07-13
    // "我希望只有 junk"): those threads live in the Junk FOLDER — the
    // sidebar's Junk entry is their one and only surface. Exposing
    // them as Inbox category tabs double-listed junk mail inside the
    // Inbox view.
    let candidates = [
        "inbox",
        "personal",
        "bulk",
        "promotions",
        "updates",
        "forums",
        "work",
        "notifications",
        "receipts",
        "newsletter",
    ];
    let categories: Vec<conv::CategoryCount> = candidates
        .into_iter()
        .map(|cat| conv::CategoryCount {
            category: cat.to_string(),
            count: state
                .mailbox
                .count_thread_ids_by_category_via_table(&_user, cat)
                .unwrap_or(0) as i64,
        })
        .filter(|c| c.count > 0)
        .collect();
    Json(conv::ConversationCategoriesResponse { categories })
}

/// `GET /v1/users/{user}/conversations/unseen-count` — a count on the
/// unread axis.
async fn get_unseen_count(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<conv::UnseenCountResponse> {
    let count = state
        .mailbox
        .count_thread_ids_by_flag_via_table(&user, "unread")
        .unwrap_or(0) as i64;
    Json(conv::UnseenCountResponse { count })
}

/// `GET /v1/users/{user}/threads/{thread_id}/messages` — returns the
/// thread's messages. mailbox-kevy doesn't store per-message rows yet
/// (only the aggregate row), so this returns the empty list until
/// Phase 7.11 lands a per-message layout. Webapi treats empty as
/// "thread exists but currently rendering, retry shortly" — graceful
/// fallback that keeps the UI from 404-ing the whole conversation
/// view while the kevy data shape grows.
async fn thread_messages(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Json<mailrs_core_api::method::thread::ListThreadMessagesResponse> {
    use mailrs_core_api::method::message::MessageWire;
    let blobs = state
        .mailbox
        .list_thread_messages(&user, &thread_id)
        .unwrap_or_default();
    let items = blobs
        .into_iter()
        .filter_map(|b| serde_json::from_slice::<MessageWire>(&b).ok())
        .collect();
    Json(mailrs_core_api::method::thread::ListThreadMessagesResponse { items })
}

/// `GET /v1/users/{user}/sent-messages` — one row per outbound message
/// (not per thread). Walks the user's sent-thread index, reads each
/// thread's messages, keeps only the ones this user actually sent, and
/// returns them newest-first with the recipient (To). Reuses the existing
/// per-thread message store — no dedicated sent-message index.
/// Every thread the user has written in, newest first.
///
/// The declared `is_sender` axis is the authority: it is maintained at
/// ingest through the membership row, and its own declaration says so —
/// "The Sent axis has the same shape: key on the flag, filter to the user,
/// sort by recency."
///
/// `user_threads_sent` is gone. That zset was legacy — it is in
/// `all_user_thread_zsets`, the list `drop-legacy-zsets` deletes — and
/// reading it was why a delivered reply was missing from Send on
/// 2026-07-30: nothing on the ingest path writes it, and its only refiller
/// was the periodic maildir sweep, which backs off exponentially while
/// idle.
///
/// It was unioned in for one release while the two sets were compared.
/// `maintenance:sent-axis-shadow` across all 13 accounts on 2026-07-31
/// reported `only_in_zset_live: 0` — the only divergence was three thread
/// ids the zset still named after a merge had emptied them, which hold no
/// messages and therefore contribute nothing to this list.
///
/// Paged rather than capped. A silent limit here would drop the oldest
/// sent threads out of the list with nothing to say it had happened.
fn sent_thread_ids(state: &Arc<FastcoreState>, user: &str) -> Vec<String> {
    const PAGE: usize = 1000;
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut offset = 0usize;
    loop {
        let page = match state.mailbox.list_thread_ids_by_flag_via_table(
            user,
            "is_sender",
            PAGE,
            offset,
            None,
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(err = %e, %user, offset, "sent axis page failed");
                break;
            }
        };
        let short = page.len() < PAGE;
        for tid in page {
            if seen.insert(tid.clone()) {
                out.push(tid);
            }
        }
        if short {
            break;
        }
        offset += PAGE;
    }

    out
}

async fn list_sent_messages(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<mailrs_core_api::method::thread::SentMessagesResponse> {
    use mailrs_core_api::method::message::MessageWire;
    use mailrs_core_api::method::thread::{SentMessageSummary, SentMessagesResponse};

    let tids = sent_thread_ids(&state, &user);

    let mut items: Vec<SentMessageSummary> = Vec::new();
    for tid in &tids {
        let tid = tid.as_str();
        let blobs = state
            .mailbox
            .list_thread_messages(&user, tid)
            .unwrap_or_default();
        for b in blobs {
            let Ok(w) = serde_json::from_slice::<MessageWire>(&b) else {
                continue;
            };
            if !mailrs_mailbox_kevy::senders_csv_contains_user(&w.sender, &user) {
                continue;
            }
            items.push(SentMessageSummary {
                uid: w.uid,
                message_id: w.message_id,
                // the thread this message is actually indexed under (the
                // merged conversation), NOT w.thread_id — a reply's stored
                // thread_id can be its own message-id-based self-thread,
                // which opens an isolated 1-message view. `tid` is what the
                // frontend resolves via get_thread_messages.
                thread_id: tid.to_string(),
                to: w.recipients,
                subject: w.subject,
                internal_date: w.internal_date,
            });
        }
    }
    items.sort_by_key(|s| std::cmp::Reverse(s.internal_date));
    Json(SentMessagesResponse { items })
}

/// `GET /v1/users/{user}/threads/by-message-id/{message_id}` — resolve a
/// RFC 5322 Message-ID to the thread it was indexed under (the msgid →
/// thread reconciliation index). Callers: webapi mirror_send, so a sent
/// reply joins the conversation its parent lives in.
async fn find_thread_by_message_id(
    State(state): State<Arc<FastcoreState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Json<mailrs_core_api::method::thread::FindThreadByMessageIdResponse> {
    let thread_id = state
        .mailbox
        .thread_for_message_id(&user, &message_id)
        .unwrap_or(None);
    Json(mailrs_core_api::method::thread::FindThreadByMessageIdResponse { thread_id })
}

/// `POST /v1/admin/backfill-threading` — one-shot rethread of existing
/// data (v2.9.5). Conversations fragmented across multiple thread_ids
/// because three write paths derived roots inconsistently and no msgid
/// index existed. Union-find over (message ↔ its In-Reply-To parent) +
/// (message ↔ its current thread) yields the true conversations; each
/// component's fragments merge into a canonical thread (the one holding
/// the component's oldest message — deterministic, so reruns are
/// idempotent no-ops). Also seeds the msgid→thread index for every
/// message. In-process per `feedback-junk-backfill-oom-finding`.
async fn backfill_threading_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    use mailrs_core_api::method::message::MessageWire;
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut merged_threads = 0u64;
    let mut moved_messages = 0u64;
    let mut indexed = 0u64;
    let mut sends_repointed = 0u64;
    // What the pass actually looked at. Without these a zero result is
    // ambiguous between "nothing to merge" and "saw nothing" — the reading
    // that cost two failed repair attempts on 2026-07-30, when
    // `msgids_indexed: 9` was the only clue that the enumeration was blind.
    let mut threads_enumerated = 0u64;
    let mut messages_seen = 0u64;
    let mut in_reply_to_edges = 0u64;
    let mut reference_edges = 0u64;
    let mut rejected_subject = 0u64;
    let mut rejected_unreadable = 0u64;
    let mut no_references = 0u64;
    let mut unreadable_samples: Vec<String> = Vec::new();
    for user in &users {
        // collect every (message_id, in_reply_to, internal_date, tid, blob_ref)
        //
        // Enumerated from the declared `threaduser` rows, not from
        // `user_threads_by_activity`. That zset is legacy — it is in
        // `all_user_thread_zsets`, which `drop-legacy-zsets` deletes — so
        // once it had been dropped this loop saw almost nothing and the
        // backfill became a no-op that still answered 200. Measured on prod
        // 2026-07-30: the zset yielded 9 messages while the table held
        // 30,562 thread-user rows, so every References edge and every merge
        // this exists to perform had silently stopped happening.
        //
        // A keyspace scan, which is what the census does for the same rows.
        // Acceptable here and nowhere near a request path: this is a
        // one-shot admin sweep over the whole mailbox by definition.
        let prefix = format!("mailrs:threaduser:{user}:");
        let tids: Vec<Vec<u8>> = store
            .keys(Some(format!("{prefix}*").as_bytes()), None)
            .into_iter()
            .filter_map(|k| {
                let k = String::from_utf8(k).ok()?;
                k.strip_prefix(&prefix).map(|tid| tid.as_bytes().to_vec())
            })
            .collect();
        let tids: Vec<(Vec<u8>, f64)> = tids.into_iter().map(|t| (t, 0.0)).collect();
        threads_enumerated += tids.len() as u64;
        let mut msgs: Vec<(String, String, i64, String, String, String)> = Vec::new();
        let mut senders_by_tid: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        // last INBOUND message per thread — display time/subject must
        // track the other side, not the user's own replies (2026-07-18).
        let mut last_inbound_by_tid: std::collections::HashMap<String, (i64, String)> =
            std::collections::HashMap::new();
        for (tid_b, _) in &tids {
            let Ok(tid) = std::str::from_utf8(tid_b) else {
                continue;
            };
            for blob in state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default()
            {
                if let Ok(w) = serde_json::from_slice::<MessageWire>(&blob) {
                    let list = senders_by_tid.entry(tid.to_string()).or_default();
                    let sender = w.sender.trim().to_string();
                    if !sender.is_empty() && !list.iter().any(|s| s.eq_ignore_ascii_case(&sender)) {
                        list.push(sender);
                    }
                    if !mailrs_mailbox_kevy::senders_csv_contains_user(&w.sender, user) {
                        let entry = last_inbound_by_tid
                            .entry(tid.to_string())
                            .or_insert((w.internal_date, w.subject.clone()));
                        if w.internal_date > entry.0 {
                            *entry = (w.internal_date, w.subject.clone());
                        }
                    }
                    msgs.push((
                        w.message_id,
                        w.in_reply_to,
                        w.internal_date,
                        tid.to_string(),
                        w.blob_ref,
                        w.subject,
                    ));
                }
            }
        }
        // Repair participant unions clobbered by the pre-fix overwrite
        // (a user's own reply used to erase every other participant).
        let mut senders_repaired = 0u64;
        let mut dates_repaired = 0u64;
        for (tid, list) in &senders_by_tid {
            let union = list.join(",");
            if let Ok(Some(mut row)) = state.mailbox.get_thread(tid) {
                let mut dirty = false;
                if row.senders_csv != union && !union.is_empty() {
                    row.senders_csv = union;
                    dirty = true;
                    senders_repaired += 1;
                }
                // own replies used to advance latest_date past the last
                // inbound message — pull the row back to inbound time.
                if let Some((date, subject)) = last_inbound_by_tid.get(tid)
                    && row.latest_date != *date
                {
                    row.latest_date = *date;
                    if !subject.is_empty() {
                        row.subject = subject.clone();
                    }
                    dirty = true;
                    dates_repaired += 1;
                }
                if dirty && state.mailbox.upsert_thread(user, &row).is_err() {
                    tracing::warn!(%user, %tid, "backfill: upsert_thread repair failed");
                }
            }
        }
        if senders_repaired > 0 || dates_repaired > 0 {
            tracing::info!(
                %user,
                senders_repaired,
                dates_repaired,
                "backfill: thread rows repaired"
            );
        }
        if msgs.is_empty() {
            continue;
        }
        // union-find over string nodes: `m:<mid>` and `t:<tid>` — a
        // message unions with its current thread, its In-Reply-To
        // parent, AND every Message-ID in its raw References chain
        // (read from the maildir file — the kevy wire doesn't store the
        // chain). Reply chains stitch fragments together while
        // already-grouped threads never split.
        messages_seen += msgs.len() as u64;
        let mut uf = UnionFind::default();
        // subject lookup so ancestry edges respect the Gmail rule: a
        // reply that changed topic must NOT glue two conversations.
        let subj_by_mid: std::collections::HashMap<&str, String> = msgs
            .iter()
            .map(|(mid, _, _, _, _, subject)| {
                (
                    mid.as_str(),
                    mailrs_mailbox_kevy::normalize_subject(subject),
                )
            })
            .collect();
        let subjects_agree =
            |a: &str, b: &str, subj_by_mid: &std::collections::HashMap<&str, String>| {
                match (subj_by_mid.get(a), subj_by_mid.get(b)) {
                    // unknown side (ancestor never ingested) → trust the edge
                    (Some(x), Some(y)) => x == y || x.is_empty() || y.is_empty(),
                    _ => true,
                }
            };
        for (mid, irt, _, tid, blob_ref, _) in &msgs {
            uf.union(&format!("m:{mid}"), &format!("t:{tid}"));
            if !irt.is_empty() {
                if subjects_agree(mid, irt, &subj_by_mid) {
                    uf.union(&format!("m:{mid}"), &format!("m:{irt}"));
                    in_reply_to_edges += 1;
                } else {
                    rejected_subject += 1;
                }
            }
            match maildir_references(user, blob_ref) {
                None => {
                    rejected_unreadable += 1;
                    // A count with no example is one step better than silence
                    // and still not actionable: prod's first legible run
                    // reported 109 unreadable against 31,566 files on disk,
                    // so nothing was missing and the cause had to be the
                    // reference itself.
                    if unreadable_samples.len() < 8 {
                        unreadable_samples.push(blob_ref.to_string());
                    }
                }
                Some(refs) if refs.is_empty() => no_references += 1,
                Some(refs) => {
                    for r in refs {
                        if subjects_agree(mid, &r, &subj_by_mid) {
                            uf.union(&format!("m:{mid}"), &format!("m:{r}"));
                            reference_edges += 1;
                        } else {
                            rejected_subject += 1;
                        }
                    }
                }
            }
        }
        // component → member tids + its oldest message's tid (canonical)
        let mut comp_tids: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut comp_oldest: std::collections::HashMap<String, (i64, String)> =
            std::collections::HashMap::new();
        for (mid, _, date, tid, _, _) in &msgs {
            let root = uf.find(&format!("m:{mid}"));
            let entry = comp_tids.entry(root.clone()).or_default();
            if !entry.contains(tid) {
                entry.push(tid.clone());
            }
            let best = comp_oldest.entry(root).or_insert((*date, tid.clone()));
            if *date < best.0 {
                *best = (*date, tid.clone());
            }
        }
        // Thread ids that stop existing, and what absorbed them. The Send
        // projection stores a thread id per send, taken at enqueue, and a
        // merge invalidates it — a row left holding a dead id navigates to
        // an empty conversation.
        let mut merged_away: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (root, tids) in &comp_tids {
            let Some((_, canonical)) = comp_oldest.get(root) else {
                continue;
            };
            for tid in tids {
                if tid != canonical {
                    match state.mailbox.merge_thread_into(user, tid, canonical) {
                        Ok(n) => {
                            merged_threads += 1;
                            moved_messages += n as u64;
                            merged_away.insert(tid.clone(), canonical.clone());
                        }
                        Err(e) => {
                            tracing::warn!(err = %e, %user, %tid, %canonical, "merge_thread_into failed");
                        }
                    }
                }
            }
        }
        // The projection lives in network kevy, the threads in the embedded
        // store, so this is a cross-store write — done here because the
        // merge is the only moment that knows which ids died.
        if !merged_away.is_empty() {
            match state.net_conn() {
                Some(mut conn) => {
                    match mailrs_core_sidestate::families::send::repoint_threads(
                        &mut conn,
                        user,
                        &merged_away,
                    ) {
                        Ok(n) => sends_repointed += n,
                        Err(e) => tracing::warn!(
                            err = %e, %user,
                            "send rows not re-pointed — Send will open an empty thread for them"
                        ),
                    }
                }
                None => tracing::warn!(
                    %user,
                    "no network kevy connection — send rows not re-pointed"
                ),
            }
        }
        // seed the msgid index for every message (merge already
        // re-pointed the moved ones; this covers the untouched rest).
        for (mid, _, _, tid, _, _) in &msgs {
            let root = uf.find(&format!("m:{mid}"));
            let Some((_, canonical)) = comp_oldest.get(&root) else {
                continue;
            };
            let target = if comp_tids.get(&root).map(|v| v.len() > 1).unwrap_or(false) {
                canonical
            } else {
                tid
            };
            if state
                .mailbox
                .set_thread_for_message_id(user, mid, target)
                .is_ok()
            {
                indexed += 1;
            }
        }
    }
    tracing::info!(
        merged_threads,
        moved_messages,
        indexed,
        "backfill-threading complete"
    );
    Json(serde_json::json!({
        // What changed.
        "merged_threads": merged_threads,
        "moved_messages": moved_messages,
        "msgids_indexed": indexed,
        "sends_repointed": sends_repointed,
        // What was looked at, so a row of zeros above is legible. All zero
        // here means the enumeration is blind, which is a different fault
        // from having nothing to do.
        "threads_enumerated": threads_enumerated,
        "messages_seen": messages_seen,
        // Which ancestry edges were found, and what was turned down.
        "in_reply_to_edges": in_reply_to_edges,
        "reference_edges": reference_edges,
        "edges_rejected": {
            // A reply that changed topic: the Gmail rule declining to glue
            // two conversations. Expected to be non-zero.
            "subject_mismatch": rejected_subject,
            // The maildir file could not be opened. Any number here is a
            // defect — it means References were unreadable, not absent.
            "file_unreadable": rejected_unreadable,
            "file_unreadable_samples": unreadable_samples,
        },
        // Read fine, names no ancestor. Not a rejection: most mail is not a
        // reply.
        "messages_without_references": no_references,
    }))
    .into_response()
}

/// Read a message's raw bytes from its maildir file. `blob_ref` is a
/// bare filename for INBOX or `.Folder/<filename>` for a Maildir++
/// subfolder; both `cur` and `new` are tried since a message moves
/// between them as flags change. `None` when the ref is empty or the
/// file is gone.
pub(crate) fn read_maildir_file(user: &str, blob_ref: &str) -> Option<Vec<u8>> {
    let (local, domain) = user.split_once('@')?;
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(root).join(domain).join(local);
    // Both the reference convention and the `:2,FLAGS` matching live in the
    // stone. This function held its own version of each: it split on `/`
    // unconditionally and rebuilt the filename by hand, so a message that
    // had been marked Seen was unreadable and the threading backfill lost
    // every `References` edge belonging to a sent copy.
    let (dir, id) = mailrs_maildir::locate(&base, blob_ref)?;
    dir.fetch(&id).ok().flatten()
}

/// Remove the maildir file at `blob_ref` — the disk counterpart of
/// `KevyMailboxStore::delete_thread`. Tries both `cur/` and `new/`
/// because a message hops between them as its `\Seen` flag flips.
///
/// Best-effort: an fs error (permission, race, already gone) logs a
/// warning but must not fail the surrounding delete — the point of
/// this helper is to prevent self-heal from resurrecting the row on
/// its next tick, and a missing file already satisfies that. Returns
/// true if any file was actually unlinked (helpful for the caller's
/// log line).
pub(crate) fn unlink_maildir_file(user: &str, blob_ref: &str) -> bool {
    if blob_ref.is_empty() {
        return false;
    }
    let Some((local, domain)) = user.split_once('@') else {
        return false;
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let base = std::path::PathBuf::from(root).join(domain).join(local);
    let (sub, name) = match blob_ref.split_once('/') {
        Some((s, n)) => (Some(s), n),
        None => (None, blob_ref),
    };
    let mut removed = false;
    for leaf in ["cur", "new"] {
        let path = match sub {
            Some(s) => base.join(s).join(leaf).join(name),
            None => base.join(leaf).join(name),
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                removed = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(
                    error = %e, path = %path.display(),
                    "delete_thread: could not unlink maildir file"
                );
            }
        }
    }
    removed
}

/// Read the full References chain of a message from its maildir file
/// (the kevy wire only stores In-Reply-To). Returns [] when the blob_ref
/// is empty or the file is gone — the caller just gets fewer edges.
fn maildir_references(user: &str, blob_ref: &str) -> Option<Vec<String>> {
    // `None` means the file could not be opened; `Some(vec![])` means it was
    // read and names no ancestor. Returning an empty vec for both hid the
    // 2026-07-30 defect for as long as it lasted: every sent copy was
    // unreadable through a hand-built filename, and the caller could not tell
    // that from "this mail is not a reply".
    let bytes = read_maildir_file(user, blob_ref)?;
    let head = &bytes[..bytes.len().min(16 * 1024)];
    let (_, _, references, ..) = extract_headers(head);
    Some(references)
}

/// `POST /v1/admin/threads:split-message` `{user, message_id}` — move a
/// message out of its thread into its own conversation (manual fix for
/// topic-change replies that were glued before the subject gate landed).
async fn split_message_route(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<serde_json::Value>,
) -> axum::response::Response {
    let user = req["user"].as_str().unwrap_or("");
    let mid = req["message_id"].as_str().unwrap_or("");
    if user.is_empty() || mid.is_empty() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    match state.mailbox.split_message_to_new_thread(user, mid) {
        Ok(Some(tid)) => Json(serde_json::json!({"thread_id": tid})).into_response(),
        Ok(None) => axum::http::StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(err = %e, %user, %mid, "split_message failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:rewrite-aof` — compact the embedded kevy
/// AOF from the CURRENT in-memory state. Recovery valve for the
/// 2026-07-17 corrupt-frame black hole: a torn frame (non-graceful
/// deploy kill) stuck mid-file meant every boot replayed only up to it
/// and appended past it — all later writes silently vanished on the
/// next restart. Rewriting emits a clean log so replay covers
/// everything again.
async fn rewrite_aof_route(State(state): State<Arc<FastcoreState>>) -> axum::response::Response {
    match state.mailbox.store_ref().rewrite_aof() {
        Ok(stats) => Json(serde_json::json!({
            "ok": true,
            "stats": format!("{stats:?}"),
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "rewrite_aof failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Minimal string-keyed union-find for the rethread backfill.
#[derive(Default)]
struct UnionFind {
    parent: std::collections::HashMap<String, String>,
}

impl UnionFind {
    fn find(&mut self, x: &str) -> String {
        let p = match self.parent.get(x) {
            Some(p) => p.clone(),
            None => {
                self.parent.insert(x.to_string(), x.to_string());
                return x.to_string();
            }
        };
        if p == x {
            return p;
        }
        let root = self.find(&p);
        self.parent.insert(x.to_string(), root.clone());
        root
    }

    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

// ── Account (auth) — Phase 8 ────────────────────────────────────────

/// `GET /v1/admin/accounts/{address}/credentials` — used by webapi's
/// login handler to fetch the argon2 hash. Blob in kevy is a JSON
/// AccountWithHashWire; we forward it verbatim.
async fn get_account_with_hash(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_account_blob(&address) {
        Ok(Some(json)) => Ok(([("content-type", "application/json")], json).into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_account_blob failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /v1/admin/accounts/{address}/effective-permissions`.
async fn effective_permissions(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_permissions_blob(&address) {
        Ok(Some(json)) => Ok(([("content-type", "application/json")], json).into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_permissions_blob failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /v1/admin/accounts` — walk kevy account index + return
/// AccountListResponse. Zero spg.
async fn list_accounts(State(state): State<Arc<FastcoreState>>) -> Json<adm::AccountListResponse> {
    let mut items = Vec::new();
    let addrs = state.mailbox.list_account_addresses().unwrap_or_default();
    for addr in addrs {
        if let Ok(Some(json)) = state.mailbox.get_account_blob(&addr)
            && let Ok(acc) = serde_json::from_str::<adm::AccountWithHashWire>(&json)
        {
            items.push(acc.public);
        }
    }
    Json(adm::AccountListResponse { items })
}

/// `GET /v1/users/{user}/messages/by-uid/{uid}` — look up a message by
/// the user-scoped uid index (populated by `deliver_message` /
/// `mailrs-fastcore-backfill-uid-index`). Returns the JSON MessageWire.
async fn get_message_by_uid_for_user(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(bytes)) => Ok((
            [("content-type", "application/json")],
            String::from_utf8(bytes).unwrap_or_default(),
        )
            .into_response()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %user, %uid, "get_message_by_uid failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ── Mailboxes (folders) ────────────────────────────────────────────

/// `GET /v1/users/{user}/mailboxes` — returns the INBOX + standard IMAP
/// folders. Counts derived from kevy zsets so no spg touch.
/// This is a minimum-viable shape — future phase populates true
/// per-mailbox metadata via mailbox-kevy `list_mailboxes` when the
/// mailbox → messages sub-index lands.
async fn list_mailboxes(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<mailrs_core_api::method::mailbox::ListMailboxesResponse> {
    use mailrs_core_api::method::mailbox::{ListMailboxesResponse, MailboxWire};
    let total = state
        .mailbox
        .count_thread_ids_by_activity_via_table(&user)
        .unwrap_or(0) as u32;
    let unseen = state
        .mailbox
        .count_thread_ids_by_flag_via_table(&user, "unread")
        .unwrap_or(0) as u32;
    let items = vec![
        MailboxWire {
            id: 1,
            user: user.clone(),
            name: "INBOX".to_string(),
            uidvalidity: 1,
            uidnext: total + 1,
            highest_modseq: total as u64,
        },
        MailboxWire {
            id: 2,
            user: user.clone(),
            name: "Sent".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 3,
            user: user.clone(),
            name: "Drafts".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 4,
            user: user.clone(),
            name: "Junk".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
        MailboxWire {
            id: 5,
            user,
            name: "Trash".to_string(),
            uidvalidity: 1,
            uidnext: 1,
            highest_modseq: 0,
        },
    ];
    let _ = unseen;
    Json(ListMailboxesResponse { items })
}

// ── Thread mutations ───────────────────────────────────────────────

/// Uniform mutation response — matches monolith's `ThreadActionResponse`
/// JSON shape so the core-rpc client's deserializer succeeds. Fastcore's
/// mutations are idempotent (mark_seen / set_pinned / set_starred / ...
/// are all noop-safe when the target thread is already in the requested
/// state or missing). Return 200 unconditionally so the UI's optimistic
/// patch never rolls back — a missing thread row simply means "nothing
/// to do" and the list refetch will reconcile.
fn action_result(_found: bool) -> axum::response::Response {
    use axum::response::IntoResponse;
    Json(th::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    })
    .into_response()
}

async fn mark_read(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    // Only a genuine unread -> read transition is an engagement event.
    // `mark_seen` returns whether the hash carried an unread_count
    // field, not whether anything changed, so the unread state has to
    // be read here. Without this a client that re-marks an open thread
    // inflates read_count, and the ranker would later learn from a
    // number that measures UI chatter rather than attention.
    let was_unread = state
        .mailbox
        .get_thread(&thread_id)
        .ok()
        .flatten()
        .is_some_and(|r| r.unread_count > 0);
    let event = crate::importance::read_event(&state, &thread_id, now_secs());
    if let Err(e) = state.mailbox.mark_seen(&user, &thread_id) {
        tracing::warn!(error = %e, %user, %thread_id, "mark_seen io error — treating as noop");
    }
    if was_unread {
        crate::importance::record_engagement(&state, &user, &thread_id, event);
    }
    action_result(true)
}

/// POST `/v1/users/{user}/conversations:mark-all-read` — sweep every
/// unread thread and flip it to seen in one call. UI's "Mark all as
/// read" button was previously batching only the loaded pagination
/// slice, so users with 99+ unread across pages left the tail
/// untouched.
async fn mark_all_read_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<serde_json::Value> {
    let flipped = state.mailbox.mark_all_seen(&user).unwrap_or(0);
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}

async fn pin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_pinned(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

async fn star_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_starred(&user, &thread_id, true)
        .unwrap_or(false);
    if ok {
        crate::importance::record_engagement(
            &state,
            &user,
            &thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::Starred,
        );
    }
    action_result(ok)
}

async fn unstar_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_starred(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn unpin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_pinned(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn archive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    // Archiving something still unread is the user dismissing it
    // unseen — the strongest implicit "not worth my attention" signal
    // there is. Read the unread state before the archive write.
    let dismissed_unread = state
        .mailbox
        .get_thread(&thread_id)
        .ok()
        .flatten()
        .is_some_and(|r| r.unread_count > 0);
    let ok = state
        .mailbox
        .set_archived(&user, &thread_id, true)
        .unwrap_or(false);
    if ok && dismissed_unread {
        crate::importance::record_engagement(
            &state,
            &user,
            &thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::ArchivedUnread,
        );
    }
    action_result(ok)
}

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as junk.
async fn mark_junk(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_junk(&user, &thread_id, true)
        .unwrap_or(false);
    if ok {
        crate::importance::record_engagement(
            &state,
            &user,
            &thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::MarkedJunk,
        );
    }
    // v2.8.0: feed the Bayesian corpus off the user's explicit junk
    // verdict (RFC 20260713). Best-effort; never blocks the move.
    if ok {
        crate::bayes_train::train_thread(&state, &user, &thread_id, true);
    }
    action_result(ok)
}

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as not junk. The
/// webapi layer separately writes to `spam:{user}:whitelist`; this
/// RPC just handles the mailbox side (move the thread + stamp
/// `category = "inbox"`).
async fn mark_not_junk(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_junk(&user, &thread_id, false)
        .unwrap_or(false);
    // v2.8.0: learn this thread as ham. train_thread unlearns any prior
    // spam training on the same thread first (mis-file correction).
    if ok {
        crate::bayes_train::train_thread(&state, &user, &thread_id, false);
    }
    action_result(ok)
}

/// v2.9 triage — move a thread into the Notifications bucket and train
/// the triage classifier on this correction.
async fn mark_notification(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_bucket(
            &user,
            &thread_id,
            mailrs_mailbox_kevy::keys::Bucket::Notifications,
        )
        .unwrap_or(false);
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "notification");
    }
    action_result(ok)
}

/// v2.9 triage — move a thread into the Promotions bucket and train
/// the triage classifier on this correction.
async fn mark_promotion(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_bucket(
            &user,
            &thread_id,
            mailrs_mailbox_kevy::keys::Bucket::Promotions,
        )
        .unwrap_or(false);
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "promotion");
    }
    action_result(ok)
}

/// v2.9 triage — move a thread back into the Inbox bucket and train the
/// triage classifier on this correction.
async fn move_to_inbox(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_bucket(&user, &thread_id, mailrs_mailbox_kevy::keys::Bucket::Inbox)
        .unwrap_or(false);
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "inbox");
    }
    action_result(ok)
}

async fn unarchive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_archived(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

async fn delete_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    // The kevy side of the delete returns the maildir blob_refs it saw
    // before wiping the message rows. Without unlinking those files
    // here, self-heal's next tick re-imports every one of them and the
    // "deleted" thread re-appears — confirmed on prod 2026-07-24 with
    // two ghost FYI threads that survived multiple UI deletes.
    let (existed, blob_refs) = match state.mailbox.delete_thread(&user, &thread_id) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, "delete_thread kevy failed");
            return action_result(false);
        }
    };
    let mut unlinked = 0u32;
    for blob_ref in &blob_refs {
        if unlink_maildir_file(&user, blob_ref) {
            unlinked += 1;
        }
    }
    if existed {
        tracing::info!(
            %user, %thread_id, messages = blob_refs.len(), unlinked,
            "delete_thread: cleared thread + unlinked maildir files"
        );
    }
    action_result(existed)
}

/// `POST /v1/users/{user}/conversations:by-thread-ids` — hydrate full
/// conversation rows for a set of thread_ids (search results),
/// preserving the requested order (G10).
async fn conversations_by_thread_ids(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ConversationsByIdsRequest>,
) -> Json<conv::ConversationsByIdsResponse> {
    let _ = user;
    let items = req
        .thread_ids
        .iter()
        .filter_map(|tid| {
            state
                .mailbox
                .get_thread(tid)
                .ok()
                .flatten()
                .map(row_to_wire)
        })
        .collect();
    Json(conv::ConversationsByIdsResponse { items })
}

use axum::response::IntoResponse;

async fn mark_unread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.mark_unread(&user, &thread_id) {
        tracing::warn!(error = %e, %user, %thread_id, "mark_unread io error — treating as noop");
    }
    action_result(true)
}

async fn snooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::SnoozeRequest>,
) -> axum::response::Response {
    if let Err(e) = state
        .mailbox
        .set_snoozed(&user, &thread_id, req.snoozed_until)
    {
        tracing::warn!(error = %e, %user, %thread_id, "snooze io error");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn unsnooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.set_snoozed(&user, &thread_id, 0) {
        tracing::warn!(error = %e, %user, %thread_id, "unsnooze io error");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// POST /v1/users/{user}/threads/{thread_id}/messages — the sent /
/// draft / import write path. Mirrors what the inbound ingest loop
/// does, but the caller controls the metadata (senders_csv, unread,
/// category) so it can synthesize a "user is the sender" arrival.
///
/// Executes 3 atomic-ish steps:
///   1. `record_message_arrival` — thread aggregate + activity/category
///      zsets + has_unread toggle if `unread=true`
///   2. `upsert_message` — write `mailrs:msg:<mid>` blob (verbatim
///      `payload_wire_json`) + zadd `mailrs:thread:<tid>:messages`
///   3. `upsert_thread` — re-read the aggregate we just updated and
///      re-emit every index, most importantly `user_threads_sent` (adds
///      when `senders_csv_contains_user`) and `has_unread`
async fn deliver_message(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::DeliverMessageRequest>,
) -> axum::response::Response {
    use mailrs_mailbox_kevy::MessageArrival;
    let arrival = MessageArrival {
        thread_id: &thread_id,
        user: &user,
        subject: &req.subject,
        senders_csv: &req.senders_csv,
        latest_date: req.latest_date,
        latest_preview: &req.latest_preview,
        category: &req.category,
        unread: req.unread,
        is_own: mailrs_mailbox_kevy::senders_csv_contains_user(&req.senders_csv, &user),
    };

    if let Err(e) = state.mailbox.record_message_arrival(&arrival) {
        tracing::error!(err = %e, %user, %thread_id, "record_message_arrival failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Side sink so contacts autocomplete stays live on webapi-
    // driven deliveries (mirror-send, forward-into-thread, etc.).
    let _ = state.notify.send(user.clone());
    crate::live_sync::publish_new_mail(
        &user,
        &thread_id,
        &req.senders_csv,
        &req.subject,
        &req.latest_preview,
    );
    crate::live_sync::upsert_contacts(&user, &req.senders_csv);

    // Allocate the per-user persistent uid HERE, not at the caller —
    // fastcore owns the uid space. mirror_send used to pass wires with
    // uid=0 straight through, so every web-sent message produced
    // /api/mail/messages/0/attachments/... URLs that 404'd (attachment
    // preview / raw / flags all resolve via the uid index).
    // allocate_uid is idempotent per (user, message_id).
    let payload = match state.mailbox.allocate_uid(&user, &req.message_id) {
        Ok(uid) if uid != 0 => {
            let _ = state.mailbox.index_uid(&user, uid, &req.message_id);
            match serde_json::from_str::<mailrs_core_api::method::message::MessageWire>(
                &req.payload_wire_json,
            ) {
                Ok(mut wire) => {
                    wire.uid = uid;
                    serde_json::to_string(&wire).unwrap_or_else(|_| req.payload_wire_json.clone())
                }
                Err(_) => req.payload_wire_json.clone(),
            }
        }
        _ => req.payload_wire_json.clone(),
    };
    if let Err(e) = state.mailbox.upsert_message(
        &thread_id,
        &req.message_id,
        req.latest_date,
        payload.as_bytes(),
    ) {
        tracing::error!(err = %e, %user, %thread_id, "upsert_message failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // register the (sent-copy) message id → thread so a remote reply
    // citing it via In-Reply-To resolves into this conversation instead
    // of opening a fragment (the v2.9.5 threading fix's key edge).
    let _ = state
        .mailbox
        .set_thread_for_message_id(&user, &req.message_id, &thread_id);

    // Re-emit thread row so index zsets (sent, has_unread, etc.) reflect
    // the new senders_csv / unread_count state. We read the row we just
    // wrote and hand it to upsert_thread which owns the index fanout.
    match state.mailbox.get_thread(&thread_id) {
        Ok(Some(row)) => {
            if let Err(e) = state.mailbox.upsert_thread(&user, &row) {
                tracing::warn!(err = %e, %user, %thread_id, "upsert_thread reindex failed");
            }
        }
        Ok(None) => {
            tracing::warn!(%user, %thread_id, "get_thread returned None right after write");
        }
        Err(e) => {
            tracing::warn!(err = %e, %user, %thread_id, "get_thread failed");
        }
    }

    if req.uid > 0
        && let Err(e) = state.mailbox.index_uid(&user, req.uid, &req.message_id)
    {
        tracing::warn!(err = %e, %user, uid = req.uid, "index_uid failed");
    }

    Json(th::DeliverMessageResponse {
        thread_id,
        message_id: req.message_id,
    })
    .into_response()
}

// ── Group B: admin write handlers ─────────────────────────────────
//
// The webapi used to write account / permission / message blobs to
// the network kevy directly (`MAILRS_KEVY_URL`). Fastcore reads its
// own embedded kevy at `/data/kevy-fastcore`, so those writes never
// affected login / account list / update_flags. These handlers close
// the gap: webapi calls fastcore RPCs, fastcore mutates its embedded
// kevy through the same `KevyMailboxStore` used at boot / ingest.

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn add_account_route(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<adm::AddAccountRequest>,
) -> axum::response::Response {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
    };
    let salt = SaltString::generate(&mut ArgonRng);
    let hash = match Argon2::default().hash_password(req.password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let domain = req
        .address
        .split_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default();
    let blob = serde_json::json!({
        "address": &req.address,
        "domain": domain,
        "display_name": req.display_name,
        "active": true,
        "created_at": now_secs(),
        "quota_bytes": 10_737_418_240i64,
        "recovery_email": null,
        "password_hash": hash,
    });
    let json = blob.to_string();
    if let Err(e) = state.mailbox.upsert_account(&req.address, &json) {
        tracing::error!(err = %e, addr = %req.address, "upsert_account failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // v2.2 (2026-07-09): domain index self-heal. The admin UI's
    // account-form + alias-form domain dropdown reads
    // `mailrs:domains:index` — if the operator provisioned an account
    // with a fresh domain we hadn't seen before, the dropdown would
    // still be missing that value until the operator remembers to
    // POST /admin/domains. Idempotent upsert.
    if !domain.is_empty()
        && let Err(e) = state.mailbox.upsert_domain(&domain, now_secs())
    {
        tracing::warn!(err = %e, %domain, "upsert_domain self-heal from add_account failed");
    }
    let perms = serde_json::json!({
        "address": &req.address,
        "permissions": Vec::<String>::new(),
        "groups": Vec::<serde_json::Value>::new(),
        "is_super": false,
        "send_as": Vec::<String>::new(),
    })
    .to_string();
    if let Err(e) = state.mailbox.upsert_permissions(&req.address, &perms) {
        tracing::warn!(err = %e, addr = %req.address, "upsert_permissions failed");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn update_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::UpdateAccountRequest>,
) -> axum::response::Response {
    let cur = match state.mailbox.get_account_blob(&address) {
        Ok(Some(s)) => s,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut val: serde_json::Value = match serde_json::from_str(&cur) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Some(obj) = val.as_object_mut() {
        obj.insert(
            "display_name".into(),
            serde_json::Value::String(req.display_name),
        );
    }
    let json = val.to_string();
    if let Err(e) = state.mailbox.upsert_account(&address, &json) {
        tracing::error!(err = %e, %address, "upsert_account failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn remove_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.delete_account(&address) {
        tracing::warn!(err = %e, %address, "delete_account failed");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn set_quota_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SetQuotaRequest>,
) -> axum::response::Response {
    crate::live_sync::mirror_quota_limit(&address, req.quota_bytes);
    patch_account_field(&state, &address, |obj| {
        obj.insert(
            "quota_bytes".into(),
            serde_json::Value::from(req.quota_bytes),
        );
    })
    .await
}

async fn set_recovery_email_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::UpdateRecoveryEmailRequest>,
) -> axum::response::Response {
    patch_account_field(&state, &address, |obj| {
        obj.insert(
            "recovery_email".into(),
            serde_json::Value::String(req.recovery_email),
        );
    })
    .await
}

async fn set_password_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SetPasswordRequest>,
) -> axum::response::Response {
    patch_account_field(&state, &address, |obj| {
        obj.insert(
            "password_hash".into(),
            serde_json::Value::String(req.password_hash),
        );
    })
    .await
}

async fn patch_account_field<F>(
    state: &Arc<FastcoreState>,
    address: &str,
    mutator: F,
) -> axum::response::Response
where
    F: FnOnce(&mut serde_json::Map<String, serde_json::Value>),
{
    let cur = match state.mailbox.get_account_blob(address) {
        Ok(Some(s)) => s,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut val: serde_json::Value = match serde_json::from_str(&cur) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Some(obj) = val.as_object_mut() {
        mutator(obj);
    }
    let json = val.to_string();
    if let Err(e) = state.mailbox.upsert_account(address, &json) {
        tracing::error!(err = %e, %address, "upsert_account failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// `POST /v1/admin/sync/reset-cursors` — reset every registered
/// user's `mailrs:sync:cursor:<user>` key so the next
/// `ingest_sync_loop` tick treats every monolith thread as "new" and
/// runs the Group F diff path to backfill missing messages.
async fn reset_sync_cursors_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let addrs = match state.mailbox.list_account_addresses() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut cleared = 0u32;
    for user in &addrs {
        let key = format!("mailrs:sync:cursor:{user}");
        if state.mailbox.store_ref().del(&[key.as_bytes()]).is_ok() {
            cleared += 1;
        }
    }
    Json(serde_json::json!({ "cleared": cleared })).into_response()
}

/// `POST /v1/admin/maintenance:bayes-bootstrap` — one-shot seed of the
/// Bayesian spam corpus from the existing Junk (spam) + Inbox (ham)
/// folders (RFC 20260713 §5). Refuses with 409 if the corpus is
/// already populated (a repeat run would double-count). Single-user:
/// the sweep runs for every account.
async fn bayes_bootstrap_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    // Single corpus-empty guard for the whole run — a per-account guard
    // (v2.8.0) let the first trained account lock out every later one.
    if crate::bayes_train::corpus_populated(&state) {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "corpus already populated" })),
        )
            .into_response();
    }
    let mut total_spam = 0u64;
    let mut total_ham = 0u64;
    for user in &users {
        let (s, h) = crate::bayes_train::bootstrap(&state, user);
        total_spam += s;
        total_ham += h;
    }
    Json(serde_json::json!({
        "spam_trained": total_spam,
        "ham_trained": total_ham,
        // Zero and zero is the shape this route returned while its rosters
        // came from zsets nothing writes — it collected nothing and answered
        // 200. `accounts` says whether there was anything to walk at all.
        "accounts": users.len(),
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:backfill-triage` — one-shot seed of the
/// v2.9 multi-class triage corpus + retroactive re-sort of existing
/// Inbox mail into Notifications / Promotions. Header-heuristic labels
/// each Inbox thread, re-files N/P out of Inbox, and trains all three
/// classes (so one-vs-rest has data for each). Idempotent. Runs for
/// every account.
async fn backfill_triage_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let (mut inbox, mut notif, mut promo) = (0u64, 0u64, 0u64);
    for user in &users {
        let (i, n, p) = crate::bayes_train::backfill_triage(&state, user);
        inbox += i;
        notif += n;
        promo += p;
    }
    Json(serde_json::json!({
        // What it filed.
        "inbox": inbox,
        "notification": notif,
        "promotion": promo,
        // What it looked at. Three zeros above with `threads_seen` also zero
        // is a blind enumeration, not a mailbox that needed no triage — the
        // distinction this route could not make until 2026-07-31, while it
        // was reading a zset nothing writes.
        "accounts": users.len(),
        "threads_seen": inbox + notif + promo,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:backfill-thread-importance` — score
/// threads that predate the feature. `?all=1` rescores every thread;
/// the default only fills in threads with no verdict yet.
async fn backfill_thread_importance_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let only_missing = q.get("all").map(String::as_str) != Some("1");
    let (scored, skipped) = crate::importance::backfill_thread_importance(&state, only_missing);
    tracing::info!(
        scored,
        skipped,
        only_missing,
        "thread importance backfill done"
    );
    axum::Json(serde_json::json!({
        "scored": scored,
        "skipped": skipped,
        // Every enumerated thread is one or the other, so this is the input
        // size: zero here means the walk saw nothing, which is a different
        // fault from a mailbox that is already fully scored.
        "threads_enumerated": scored + skipped,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:backfill-contact-relationships` —
/// one-shot rebuild of the per-sender received/sent counters that
/// importance scoring reads. Runs in process: a `docker exec` helper
/// would open a second embedded kevy and replay the AOF alongside the
/// live one (the 2026-07-13 backfill OOM).
async fn backfill_contact_relationships_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let (users, addresses, messages) = crate::importance::backfill_relationships(&state);
    tracing::info!(users, addresses, messages, "relationship backfill complete");
    axum::Json(serde_json::json!({
        "users": users,
        "addresses": addresses,
        "messages_scanned": messages,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:shadow-read` — compare the ORDERPATH's
/// answer with the zset's for every account, without serving either.
///
/// The zsets stay authoritative through this phase. This is the only
/// step that can show the two agree on **content and order** before a
/// read is cut over; `TABLE.VERIFY` proves the index matches the rows,
/// which is a different claim — the rows themselves could be wrong.
///
/// Divergence is reported per user rather than summed, because one
/// account disagreeing is a different problem from all of them.
async fn shadow_read_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut report = Vec::new();
    let mut total_divergent = 0u64;

    for user in &users {
        // The three remaining shapes: Sent (its own flag index), the
        // default recency axis (a pure ORDERPATH), and np (a merge of
        // two bucket ranges, the only order this code produces).
        for axis in ["sent", "default", "np"] {
            let zkey = match axis {
                "sent" => mailrs_mailbox_kevy::keys::user_threads_sent(user),
                _ => mailrs_mailbox_kevy::keys::user_threads_by_activity(user),
            };
            let zset: Vec<String> = if axis == "np" {
                let mut merged: Vec<(i64, String)> = Vec::new();
                for k in [
                    mailrs_mailbox_kevy::keys::user_threads_notifications(user),
                    mailrs_mailbox_kevy::keys::user_threads_promotions(user),
                ] {
                    if let Ok(e) = store.zrevrange(k.as_bytes(), 0, limit as i64 - 1) {
                        merged.extend(e.into_iter().filter_map(|(m, sc)| {
                            String::from_utf8(m).ok().map(|t| (sc as i64, t))
                        }));
                    }
                }
                merged.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                merged.into_iter().map(|(_, t)| t).take(limit).collect()
            } else {
                match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                    Ok(e) => e
                        .into_iter()
                        .filter_map(|(m, _)| String::from_utf8(m).ok())
                        .collect(),
                    Err(_) => continue,
                }
            };
            let table = match axis {
                "sent" => state.mailbox.list_thread_ids_by_flag_via_table(
                    user,
                    "is_sender",
                    limit,
                    0,
                    None,
                ),
                "default" => state
                    .mailbox
                    .list_thread_ids_by_activity_via_table(user, limit, None),
                _ => {
                    let mut m: Vec<String> = Vec::new();
                    for b in ["notifications", "promotions"] {
                        if let Ok(t) = state
                            .mailbox
                            .list_thread_ids_by_bucket_via_table(user, b, limit)
                        {
                            m.extend(t);
                        }
                    }
                    Ok(m)
                }
            };
            let Ok(table) = table else { continue };
            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_z = zs.difference(&ts).count();
            let only_t = ts.difference(&zs).count();
            // For Sent, show what the rows actually say about the
            // threads the zset claims and the table does not — the
            // 58-vs-9 gap on one account is unexplained and the
            // membership row is where the answer has to be.
            let detail: Vec<serde_json::Value> = if axis == "sent" {
                zs.difference(&ts)
                    .take(4)
                    .map(|tid| {
                        let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                        let row = store.hgetall(key.as_bytes()).unwrap_or_default();
                        let f = |n: &str| -> Option<String> {
                            row.iter()
                                .find(|(k, _)| k == n.as_bytes())
                                .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                        };
                        let th = store
                            .hgetall(mailrs_mailbox_kevy::keys::thread(tid).as_bytes())
                            .unwrap_or_default();
                        let tf = |n: &str| -> Option<String> {
                            th.iter()
                                .find(|(k, _)| k == n.as_bytes())
                                .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                        };
                        serde_json::json!({
                            "tid": tid,
                            "row_exists": !row.is_empty(),
                            "row_is_sender": f("is_sender"),
                            "row_sent_only": f("sent_only"),
                            "thread_senders": tf("senders_csv"),
                            "thread_sent_count": tf("sent_count"),
                            "thread_count": tf("count"),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

            // np's table side is unsorted here (two concatenated
            // ranges); only membership is meaningful for it.
            let order_matches = axis == "np" || zset == table;
            if only_z > 0 || only_t > 0 || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
                    "user": user,
                    "bucket": format!("axis:{axis}"),
                    "zset_len": zset.len(),
                    "table_len": table.len(),
                    "truncated": zset.len() >= limit || table.len() >= limit,
                    "only_in_zset": only_z,
                    "only_in_table": only_t,
                    "order_matches": order_matches,
                    "zset_only_rows": detail,
                    "table_only_rows": [],
                    "order_diff": serde_json::Value::Null,
                }));
            }
        }

        // Boolean predicate axes — each served from its own flag
        // index rather than a sort prefix.
        for (flag, zkey) in [
            (
                "starred",
                mailrs_mailbox_kevy::keys::user_threads_starred(user),
            ),
            (
                "archived",
                mailrs_mailbox_kevy::keys::user_threads_archived(user),
            ),
            (
                "pinned",
                mailrs_mailbox_kevy::keys::user_threads_pinned(user),
            ),
            (
                "unread",
                mailrs_mailbox_kevy::keys::user_threads_has_unread(user),
            ),
            (
                "has_action",
                mailrs_mailbox_kevy::keys::user_threads_has_action(user),
            ),
        ] {
            let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                Ok(e) => e
                    .into_iter()
                    .filter_map(|(m, _)| String::from_utf8(m).ok())
                    .collect(),
                Err(_) => continue,
            };
            let table = match state
                .mailbox
                .list_thread_ids_by_flag_via_table(user, flag, limit, 0, None)
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(err = %e, %user, flag, "flag index query failed");
                    continue;
                }
            };
            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_z = zs.difference(&ts).count();
            let only_t = ts.difference(&zs).count();
            let order_matches = zset == table;
            if only_z > 0 || only_t > 0 || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
                    "user": user,
                    "bucket": format!("flag:{flag}"),
                    "zset_len": zset.len(),
                    "table_len": table.len(),
                    "truncated": zset.len() >= limit || table.len() >= limit,
                    "only_in_zset": only_z,
                    "only_in_table": only_t,
                    "order_matches": order_matches,
                    "zset_only_rows": [],
                    "table_only_rows": [],
                    "order_diff": serde_json::Value::Null,
                }));
            }
        }

        // Category axis — the other declared ORDERPATH. Distinct from
        // the bucket axis: `bucket` is the folder a thread is filed
        // under, `category` is the classifier's verdict, and the two
        // use different vocabularies (spam vs junk, notification vs
        // notifications).
        for cat in ["inbox", "notification", "promotion", "spam"] {
            let zkey = mailrs_mailbox_kevy::keys::user_threads_by_category(user, cat);
            let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                Ok(e) => e
                    .into_iter()
                    .filter_map(|(m, _)| String::from_utf8(m).ok())
                    .collect(),
                Err(_) => continue,
            };
            let table = match state
                .mailbox
                .list_thread_ids_by_category_via_table(user, cat, limit)
            {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(err = %e, %user, cat, "category orderpath query failed");
                    continue;
                }
            };
            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_z = zs.difference(&ts).count();
            let only_t = ts.difference(&zs).count();
            let order_matches = zset == table;
            if only_z > 0 || only_t > 0 || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
                    "user": user,
                    "bucket": format!("cat:{cat}"),
                    "zset_len": zset.len(),
                    "table_len": table.len(),
                    "truncated": zset.len() >= limit || table.len() >= limit,
                    "only_in_zset": only_z,
                    "only_in_table": only_t,
                    "order_matches": order_matches,
                    "zset_only_rows": [],
                    "table_only_rows": [],
                    "order_diff": serde_json::Value::Null,
                }));
            }
        }

        for (bucket, zkey) in [
            ("inbox", mailrs_mailbox_kevy::keys::user_threads_inbox(user)),
            ("junk", mailrs_mailbox_kevy::keys::user_threads_junk(user)),
            (
                "notifications",
                mailrs_mailbox_kevy::keys::user_threads_notifications(user),
            ),
            (
                "promotions",
                mailrs_mailbox_kevy::keys::user_threads_promotions(user),
            ),
        ] {
            let zset: Vec<String> = match store.zrevrange(zkey.as_bytes(), 0, limit as i64 - 1) {
                Ok(e) => e
                    .into_iter()
                    .filter_map(|(m, _)| String::from_utf8(m).ok())
                    .collect(),
                Err(e) => {
                    tracing::warn!(err = %e, %user, bucket, "zrevrange failed");
                    continue;
                }
            };
            // Inbox is served from the sent-excluding ORDERPATH, so
            // compare against that one — otherwise this reports a
            // divergence the serving path does not have.
            let table_result = if bucket == "inbox" {
                state
                    .mailbox
                    .list_thread_ids_by_bucket_unsent_via_table(user, bucket, limit)
            } else {
                state
                    .mailbox
                    .list_thread_ids_by_bucket_via_table(user, bucket, limit)
            };
            let table = match table_result {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(err = %e, %user, bucket, "orderpath query failed");
                    continue;
                }
            };

            // Both sides capped at `limit`, so a full page on either
            // side means the tails were never compared — the sets can
            // differ purely from where each was cut. Say so rather than
            // reporting a divergence the data does not support.
            let truncated = zset.len() >= limit || table.len() >= limit;

            let zs: std::collections::BTreeSet<&String> = zset.iter().collect();
            let ts: std::collections::BTreeSet<&String> = table.iter().collect();
            let only_zset: Vec<&String> = zs.difference(&ts).copied().collect();
            let only_table: Vec<&String> = ts.difference(&zs).copied().collect();
            let order_matches = zset == table;

            // When the sets agree but the order does not, the useful
            // question is where they first part and whether the two
            // threads there share an activity timestamp — a tie the
            // two sides break differently is harmless, a genuine
            // ordering difference is not.
            let order_diff = if order_matches {
                serde_json::Value::Null
            } else {
                let at = zset.iter().zip(table.iter()).position(|(a, b)| a != b);
                match at {
                    Some(i) => {
                        let act = |tid: &String| -> Option<String> {
                            let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                            store.hgetall(key.as_bytes()).ok().and_then(|row| {
                                row.iter()
                                    .find(|(f, _)| f == b"activity")
                                    .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                            })
                        };
                        serde_json::json!({
                            "at_index": i,
                            "zset_activity": act(&zset[i]),
                            "table_activity": act(&table[i]),
                        })
                    }
                    None => serde_json::json!({ "at_index": "length only" }),
                }
            };

            // A thread the zset claims and the table does not is the
            // only shape that could lose data on cutover. Report what
            // the membership row actually says about it, so a stale
            // zset entry is distinguishable from a missing row.
            let missing: Vec<serde_json::Value> = only_zset
                .iter()
                .take(5)
                .map(|tid| {
                    let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                    let row = store.hgetall(key.as_bytes()).unwrap_or_default();
                    let field = |name: &str| -> Option<String> {
                        row.iter()
                            .find(|(f, _)| f == name.as_bytes())
                            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                    };
                    serde_json::json!({
                        "tid": tid,
                        "row_exists": !row.is_empty(),
                        "row_bucket": field("bucket"),
                        "row_category": field("category"),
                    })
                })
                .collect();

            // Symmetric detail for the other direction: what the table
            // has and the zset does not. On the Inbox axis this is the
            // question of whether the rows include sent-only threads
            // the zset deliberately excludes.
            let extra: Vec<serde_json::Value> = only_table
                .iter()
                .take(5)
                .map(|tid| {
                    let key = mailrs_mailbox_kevy::keys::thread_user(user, tid);
                    let row = store.hgetall(key.as_bytes()).unwrap_or_default();
                    let field = |name: &str| -> Option<String> {
                        row.iter()
                            .find(|(f, _)| f == name.as_bytes())
                            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
                    };
                    serde_json::json!({
                        "tid": tid,
                        "sent": field("sent"),
                        "category": field("category"),
                        "archived": field("archived"),
                    })
                })
                .collect();

            if !only_zset.is_empty() || !only_table.is_empty() || !order_matches {
                total_divergent += 1;
                report.push(serde_json::json!({
                    "user": user,
                    "bucket": bucket,
                    "zset_len": zset.len(),
                    "table_len": table.len(),
                    "truncated": truncated,
                    "only_in_zset": only_zset.len(),
                    "only_in_table": only_table.len(),
                    "order_matches": order_matches,
                    "order_diff": order_diff,
                    "zset_only_rows": missing,
                    "table_only_rows": extra,
                }));
            }
        }
    }

    Json(serde_json::json!({
        "limit": limit,
        "axes_checked": users.len() * 16,
        "axes_divergent": total_divergent,
        "divergences": report,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:tier-info` — what the store is holding
/// in RAM versus on disk.
///
/// `cold_keys` / `cold_bytes` are what tiering moved out; `stub_bytes`
/// is what that cost to keep addressable. With tiering off the budget
/// reads 0 and nothing is cold, which is the honest baseline to
/// compare against.
async fn tier_info_route(State(state): State<Arc<FastcoreState>>) -> axum::response::Response {
    let info = state.mailbox.store_ref().info();
    Json(serde_json::json!({
        "keys": info.keys,
        "used_memory": info.used_memory,
        "aof_bytes": info.aof_bytes,
        // None when tiering is off — the difference between "nothing
        // is cold" and "nothing can be cold" is worth keeping visible.
        "tier": info.tiering.map(|t| {
            serde_json::json!({
                "budget_bytes": t.tier_budget_bytes,
                "effective_target": t.tier_effective_target,
                "cold_keys": t.cold_keys,
                "cold_bytes": t.cold_bytes,
                "stub_bytes": t.stub_bytes,
                "index_reserved_bytes": t.index_reserved_bytes,
                "vlog_size_bytes": t.vlog_size_bytes,
            })
        }),
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:drop-legacy-zsets` — delete the
/// hand-maintained per-user thread indexes.
///
/// Every axis is served from the declared table and no write path
/// touches these keys, so they are dead weight held in memory. Runs
/// in-process rather than through a second store handle: opening the
/// embedded store twice replays the AOF twice and gets the container
/// OOM-killed.
///
/// `?dry=1` reports what would go without deleting it.
async fn drop_legacy_zsets_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let dry = q.get("dry").map(String::as_str) == Some("1");
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut found = 0u64;
    let mut members = 0u64;
    let mut deleted = 0u64;
    for user in &users {
        for key in mailrs_mailbox_kevy::keys::all_user_thread_zsets(user) {
            let n = store.zcard(key.as_bytes()).unwrap_or(0);
            if n == 0 {
                continue;
            }
            found += 1;
            members += n as u64;
            if !dry {
                deleted += store.del(&[key.as_bytes()]).unwrap_or(0) as u64;
            }
        }
    }
    tracing::info!(dry, found, members, deleted, "legacy zset sweep");
    Json(serde_json::json!({
        "dry_run": dry,
        "keys_found": found,
        "members_held": members,
        "keys_deleted": deleted,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:threaduser-census` — walk every
/// membership row and report which ones could not be indexed.
///
/// `TABLE.VERIFY` says how many entries an index holds; when that is
/// short of the row count it does not say *which* rows are missing. A
/// composite orderpath can only encode a row where every sort column is
/// present, so this counts rows by which column is empty.
async fn threaduser_census_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let store = state.mailbox.store_ref();
    let keys = store.keys(Some(b"mailrs:threaduser:*"), None);
    let mut total = 0u64;
    let mut empty: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();
    for k in &keys {
        total += 1;
        let pairs = match store.hgetall(k) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let map: std::collections::HashMap<&[u8], &[u8]> = pairs
            .iter()
            .map(|(f, v)| (f.as_slice(), v.as_slice()))
            .collect();
        let mut bad = false;
        for col in [
            "user", "tid", "bucket", "category", "activity", "starred", "archived",
        ] {
            if map
                .get(col.as_bytes())
                .map(|v| v.is_empty())
                .unwrap_or(true)
            {
                *empty.entry(col.to_string()).or_default() += 1;
                bad = true;
            }
        }
        if bad && samples.len() < 5 {
            samples.push(String::from_utf8_lossy(k).into_owned());
        }
    }
    Json(serde_json::json!({
        "rows": total,
        "empty_or_missing": empty,
        "samples": samples,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:table-verify` — ask the engine whether
/// the access paths it maintains agree with the rows.
///
/// `drift` is the number that matters: non-zero means an index no
/// longer re-derives to what it stores, which is the failure this whole
/// migration exists to make impossible by hand. `coerce_failures` and
/// `type_mismatches` say a column's declared type does not match what
/// was written — a schema bug rather than a maintenance one.
///
/// kevy returns six unnamed counters per index; the names are recovered
/// here from the doc comment on `TableVerifyReport`.
async fn table_verify_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let name = q.get("table").map(String::as_str).unwrap_or("threaduser");
    match state.mailbox.store_ref().table_verify(name.as_bytes()) {
        Ok((per_index, spot)) => {
            let indexes: Vec<serde_json::Value> = per_index
                .into_iter()
                .map(|(n, c)| {
                    serde_json::json!({
                        "index": String::from_utf8_lossy(&n),
                        "entries": c[0],
                        "bytes": c[1],
                        "coerce_failures": c[2],
                        "duplicates": c[3],
                        "drift": c[4],
                        "checked": c[5],
                    })
                })
                .collect();
            Json(serde_json::json!({
                "table": name,
                "indexes": indexes,
                "spot_check": { "rows": spot[0], "type_mismatches": spot[1] },
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, %name, "table_verify failed");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:backfill-thread-user` — one segment of
/// the membership-row backfill for the declared `threaduser` table.
///
/// Call with `?user=<addr>&offset=<n>&limit=<n>`; omit `user` to get the
/// account list back and nothing else, which is how a driver discovers
/// what to iterate. Each call walks one page of that user's by_activity
/// zset and writes the membership row **only where it is absent or a
/// field differs**, so re-running over converged data reports
/// `written: 0` and costs one HGETALL per row rather than a write.
///
/// `done` is true when the page came back short, meaning the caller has
/// reached the end of this user's threads.
/// `POST /v1/admin/maintenance:legacy-zset-census`
///
/// Cardinality of every zset in `keys::all_user_thread_zsets` — the list
/// `drop-legacy-zsets` deletes — per account.
///
/// Read-only, and it exists because "is this reader looking at an empty
/// set?" was being answered by reasoning. `backfill-threading` enumerated
/// `user_threads_by_activity` and saw 9 messages against 30,562 declared
/// rows; `list_sent_messages` read `user_threads_sent`, which still holds
/// hundreds because the maildir sweep refills that one and nothing refills
/// the others. Those two facts together say the legacy zsets are alive to
/// different degrees, and several readers remain — in `importance.rs`,
/// `bayes_train.rs`, `backfill_decode.rs`. This turns that into numbers
/// before anything is concluded about them.
async fn legacy_zset_census_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let users = match q.get("user") {
        Some(u) => vec![u.clone()],
        None => state.mailbox.list_account_addresses().unwrap_or_default(),
    };
    let store = state.mailbox.store_ref();

    // key suffix -> total across accounts, so an all-zero row names a set
    // nothing writes any more.
    let mut totals: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut per_user = Vec::new();
    for user in &users {
        let mut row = serde_json::Map::new();
        let mut any = 0u64;
        for key in mailrs_mailbox_kevy::keys::all_user_thread_zsets(user) {
            let n = store.zcard(key.as_bytes()).unwrap_or(0) as u64;
            let label = key
                .rsplit_once(":threads:")
                .map(|(_, t)| t.to_string())
                .unwrap_or_else(|| key.clone());
            *totals.entry(label.clone()).or_default() += n;
            any += n;
            row.insert(label, serde_json::json!(n));
        }
        if any > 0 {
            row.insert("user".into(), serde_json::json!(user));
            per_user.push(serde_json::Value::Object(row));
        }
    }

    Json(serde_json::json!({
        "users_checked": users.len(),
        "totals": totals,
        "users_with_any": per_user,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:sent-axis-shadow?user=`
///
/// Compares the declared `is_sender` axis against the legacy
/// `user_threads_sent` zset, before the read path stops unioning them.
///
/// Both directions are reported separately because they mean opposite
/// things. `only_in_zset` is history the declared axis would lose if the
/// union were dropped now — it must be zero before that happens.
/// `only_in_axis` is the defect being fixed showing up as intended: the
/// zset has no ingest writer, so a send made since the last sweep is
/// expected to appear here.
///
/// Totals are reported alongside, so a pair of zeros cannot be confused
/// with having looked at nothing — the failure that made
/// `backfill-threading` answer `msgids_indexed: 9` and look fine.
async fn sent_axis_shadow_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let users = match q.get("user") {
        Some(u) => vec![u.clone()],
        None => state.mailbox.list_account_addresses().unwrap_or_default(),
    };

    let mut report = Vec::new();
    for user in &users {
        let mut axis: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut offset = 0usize;
        loop {
            let page = state
                .mailbox
                .list_thread_ids_by_flag_via_table(user, "is_sender", 1000, offset, None)
                .unwrap_or_default();
            let short = page.len() < 1000;
            axis.extend(page);
            if short {
                break;
            }
            offset += 1000;
        }

        let zset: std::collections::HashSet<String> = state
            .mailbox
            .store_ref()
            .zrevrange(
                mailrs_mailbox_kevy::keys::user_threads_sent(user).as_bytes(),
                0,
                -1,
            )
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(b, _)| String::from_utf8(b).ok())
            .collect();

        let mut only_in_zset: Vec<&String> = zset.difference(&axis).collect();
        let mut only_in_axis: Vec<&String> = axis.difference(&zset).collect();
        only_in_zset.sort();
        only_in_axis.sort();

        // A thread id in the zset that no longer holds any message is not
        // history the axis is missing — it is a dead reference the zset kept
        // after a merge emptied the thread. Counting it as divergence would
        // block the cutover forever on entries whose loss costs nothing,
        // and eyeballing which is which is exactly the judgment call this
        // gate exists to remove.
        let live = |tid: &str| {
            !state
                .mailbox
                .list_thread_messages(user, tid)
                .unwrap_or_default()
                .is_empty()
        };
        let (only_in_zset_live, only_in_zset_dead): (Vec<&String>, Vec<&String>) =
            only_in_zset.iter().partition(|t| live(t.as_str()));

        if !only_in_zset_live.is_empty() {
            tracing::warn!(
                %user, count = only_in_zset_live.len(),
                "live threads the legacy sent zset holds and the declared axis does not — \
                 dropping the union now would lose them"
            );
        }

        report.push(serde_json::json!({
            "user": user,
            "axis_threads": axis.len(),
            "zset_threads": zset.len(),
            // The gate: threads that still hold messages and are absent from
            // the axis. Must be zero before the union is dropped.
            "only_in_zset_live": only_in_zset_live.len(),
            // Dead references the zset kept. Not a blocker; they disappear
            // with the zset.
            "only_in_zset_dead": only_in_zset_dead.len(),
            "only_in_axis": only_in_axis.len(),
            "only_in_zset_live_samples": only_in_zset_live.iter().take(8).collect::<Vec<_>>(),
            "only_in_zset_dead_samples": only_in_zset_dead.iter().take(8).collect::<Vec<_>>(),
            "only_in_axis_samples": only_in_axis.iter().take(8).collect::<Vec<_>>(),
        }));
    }

    Json(serde_json::json!({ "users_checked": users.len(), "report": report })).into_response()
}

/// `POST /v1/admin/maintenance:shadow-counts?user=&offset=&limit=`
///
/// Compares the shared thread row's counters against this user's own
/// before the read path moves to the latter (RFC 20260730 S3).
///
/// `diverged_single` is the number to act on: a thread only this
/// account holds, where the two copies disagree, means a writer updated
/// one and not the other. It has to be zero before S4 flips the read.
/// `diverged_shared` is the double-counting being fixed showing up as
/// intended, and is expected to be non-zero.
async fn shadow_counts_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let Some(user) = q.get("user") else {
        let users = state.mailbox.list_account_addresses().unwrap_or_default();
        return Json(serde_json::json!({ "users": users })).into_response();
    };
    let offset: i64 = q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);

    match state.mailbox.shadow_thread_counts(user, offset, limit) {
        Ok(r) => {
            if r.diverged_single > 0 {
                tracing::warn!(
                    %user, diverged_single = r.diverged_single,
                    "thread counter copies disagree on threads only this account holds"
                );
            }
            Json(serde_json::json!({
                "user": user,
                "offset": offset,
                "limit": limit,
                "scanned": r.scanned,
                "agreed": r.agreed,
                "diverged_single": r.diverged_single,
                "diverged_shared": r.diverged_shared,
                "samples": r.samples,
                "done": (r.scanned as i64) < limit,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, %user, "shadow count report failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:recount-threads?user=&offset=&limit=`
///
/// Rebuilds `count` / `unread_count` / `sent_count` from the per-thread
/// message index. Idempotent — a second pass over converged data
/// reports `repaired: 0`. Omit `user` to list the accounts to sweep.
///
/// `shared` counts threads held by more than one local account, where a
/// global row cannot represent both views and the sweeping user's wins.
async fn recount_threads_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let Some(user) = q.get("user") else {
        let users = state.mailbox.list_account_addresses().unwrap_or_default();
        return Json(serde_json::json!({ "users": users })).into_response();
    };
    let offset: i64 = q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);

    match state.mailbox.backfill_thread_counts(user, offset, limit) {
        Ok(r) => {
            if r.repaired > 0 {
                tracing::info!(
                    %user, offset, scanned = r.scanned, repaired = r.repaired,
                    shared = r.shared, "thread counter recount segment"
                );
            }
            Json(serde_json::json!({
                "user": user,
                "offset": offset,
                "limit": limit,
                "scanned": r.scanned,
                "repaired": r.repaired,
                "shared": r.shared,
                "done": (r.scanned as i64) < limit,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, %user, "thread counter recount failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn backfill_thread_user_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let Some(user) = q.get("user") else {
        let users = state.mailbox.list_account_addresses().unwrap_or_default();
        return Json(serde_json::json!({ "users": users })).into_response();
    };
    let offset: i64 = q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);

    match state.mailbox.backfill_thread_user(user, offset, limit) {
        Ok((scanned, written)) => {
            if written > 0 {
                tracing::info!(%user, offset, scanned, written, "threaduser backfill segment");
            }
            Json(serde_json::json!({
                "user": user,
                "offset": offset,
                "limit": limit,
                "scanned": scanned,
                "written": written,
                "done": (scanned as i64) < limit,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, %user, "threaduser backfill failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:sweep-legacy-admin-keys` — one-shot
/// in-process cleanup of the pre-P6 admin keyspace (roadmap Phase
/// 11.2's embedded half, executed as an RPC per
/// `feedback-junk-backfill-oom-finding`: a `docker exec` sweep binary
/// would double-open the embedded kevy and OOM replaying the AOF;
/// running inside the live fastcore process costs nothing).
///
/// Deletes:
///   - `mailrs:alias:<addr>` legacy strings (NOT `mailrs:alias:v2:*`)
///   - `mailrs:domain:<name>` legacy strings (NOT `mailrs:domain:v2:*`)
///   - `mailrs:aliases:index` / `mailrs:domains:index` /
///     `mailrs:accounts:index` legacy sets
///
/// Idempotent — a second call finds nothing and returns zeros. No
/// reader has touched these keys since v2.6.2 (Phase 11.3 removed the
/// last code references); they only weigh down the AOF.
async fn sweep_legacy_admin_keys_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let store = state.mailbox.store_ref();
    let mut aliases = 0u32;
    let mut domains = 0u32;

    let (_, alias_keys) = store.scan(0, Some(b"mailrs:alias:*"), usize::MAX);
    let alias_keys_scanned = alias_keys.len() as u64;
    for key in alias_keys {
        if key.starts_with(b"mailrs:alias:v2:") {
            continue;
        }
        if store.del(&[key.as_slice()]).unwrap_or(0) > 0 {
            aliases += 1;
        }
    }

    let (_, domain_keys) = store.scan(0, Some(b"mailrs:domain:*"), usize::MAX);
    let domain_keys_scanned = domain_keys.len() as u64;
    for key in domain_keys {
        if key.starts_with(b"mailrs:domain:v2:") {
            continue;
        }
        if store.del(&[key.as_slice()]).unwrap_or(0) > 0 {
            domains += 1;
        }
    }

    let indexes = store
        .del(&[
            b"mailrs:aliases:index".as_slice(),
            b"mailrs:domains:index".as_slice(),
            b"mailrs:accounts:index".as_slice(),
        ])
        .unwrap_or(0);

    tracing::info!(
        aliases,
        domains,
        indexes,
        "legacy admin keyspace sweep complete"
    );
    Json(serde_json::json!({
        // What it removed.
        "legacy_alias_strings": aliases,
        "legacy_domain_strings": domains,
        "legacy_index_sets": indexes,
        // What it scanned. Zeros above with these also zero means the scan
        // matched nothing — a different fact from "no legacy keys remain".
        "alias_keys_scanned": alias_keys_scanned,
        "domain_keys_scanned": domain_keys_scanned,
    }))
    .into_response()
}

/// `POST /v1/users/{user}/messages/{uid}/flags` — patch the flags
/// bitmask on a message blob. Also reconciles the thread's `has_unread`
/// zset via `mark_seen` / `mark_unread` when `\Seen` toggled.
async fn set_message_flags_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
    Json(req): Json<adm::SetMessageFlagsRequest>,
) -> axum::response::Response {
    let bytes = match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(b)) => b,
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    let mut wire: mailrs_core_api::method::message::MessageWire =
        match serde_json::from_slice(&bytes) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(err = %e, %user, %uid, "wire parse failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    let old_flags = wire.flags;
    let new_flags = req.flags;
    wire.flags = new_flags;
    let json = match serde_json::to_vec(&wire) {
        Ok(v) => v,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(e) =
        state
            .mailbox
            .upsert_message(&wire.thread_id, &wire.message_id, wire.date, &json)
    {
        tracing::error!(err = %e, %user, %uid, "upsert_message failed");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let seen_bit = 0b0000_0001u32;
    let was_seen = (old_flags & seen_bit) != 0;
    let is_seen = (new_flags & seen_bit) != 0;
    if was_seen != is_seen && !wire.thread_id.is_empty() {
        let _ = if is_seen {
            state.mailbox.mark_seen(&user, &wire.thread_id)
        } else {
            state.mailbox.mark_unread(&user, &wire.thread_id)
        };
        // Reading over IMAP is still reading. Without this, engagement
        // would only ever be recorded for the web UI, and a user on
        // Apple Mail or Thunderbird would look like they never open
        // anything — a systematic hole in the data the ranker learns
        // from, invisible until the learner started producing nonsense.
        //
        // `was_seen != is_seen` already makes this a genuine unread ->
        // read transition, so re-syncing an unchanged flag records
        // nothing.
        if is_seen {
            let event = crate::importance::read_event(&state, &wire.thread_id, now_secs());
            crate::importance::record_engagement(&state, &user, &wire.thread_id, event);
        }
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// GET `/v1/admin/aliases:local` — list every fastcore-embedded alias.
async fn list_local_aliases(State(state): State<Arc<FastcoreState>>) -> Json<serde_json::Value> {
    let items = state.alias_store.list().unwrap_or_default();
    let payload: Vec<serde_json::Value> = items
        .into_iter()
        .map(|(source, target)| serde_json::json!({"source": source, "target": target}))
        .collect();
    Json(serde_json::json!({ "items": payload }))
}

#[derive(serde::Deserialize)]
struct AliasBody {
    source: String,
    target: String,
}

/// POST `/v1/admin/aliases:local` — insert/replace one alias entry.
async fn upsert_local_alias(
    State(state): State<Arc<FastcoreState>>,
    Json(body): Json<AliasBody>,
) -> axum::response::Response {
    if body.source.is_empty() || body.target.is_empty() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    match state.alias_store.upsert(&body.source, &body.target) {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(err = %e, source = %body.source, "upsert_alias failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE `/v1/admin/aliases:local/{source}` — drop one alias entry.
async fn delete_local_alias_route(
    State(state): State<Arc<FastcoreState>>,
    Path(source): Path<String>,
) -> axum::response::Response {
    match state.alias_store.delete(&source) {
        Ok(_) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(err = %e, %source, "delete_alias failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {

    /// A message that has been read lives in `cur/` with a `:2,FLAGS`
    /// suffix, and `read_maildir_file` used to reconstruct the filename by
    /// hand and miss it. That made the threading backfill's References
    /// edges invisible for every sent copy — `mirror_send` marks those Seen
    /// — so conversations that should have merged did not (2026-07-30).
    #[test]
    fn read_maildir_file_finds_a_flagged_message() {
        let tmp = std::env::temp_dir().join(format!("mailrs-rmf-{}", std::process::id()));
        let box_dir = tmp.join("x.com").join("bob");
        std::fs::create_dir_all(box_dir.join("cur")).unwrap();
        std::fs::create_dir_all(box_dir.join("new")).unwrap();

        // Unflagged, still in new/ — the case that already worked.
        std::fs::write(box_dir.join("new").join("plain.id"), b"raw-new").unwrap();
        // Read, so renamed into cur/ with a flag suffix.
        std::fs::write(box_dir.join("cur").join("seen.id:2,S"), b"raw-seen").unwrap();

        // SAFETY-adjacent: the env var is read inside the function under
        // test, and this is the only test that sets it.
        unsafe { std::env::set_var("MAILRS_MAILDIR", &tmp) };

        assert_eq!(
            read_maildir_file("bob@x.com", "plain.id").as_deref(),
            Some(&b"raw-new"[..]),
        );
        assert_eq!(
            read_maildir_file("bob@x.com", "seen.id").as_deref(),
            Some(&b"raw-seen"[..]),
            "a flagged file must be found by its base id"
        );
        assert!(read_maildir_file("bob@x.com", "absent.id").is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use mailrs_mailbox_kevy::MessageArrival;
    use tower::ServiceExt;

    pub(super) fn fresh_state() -> Arc<FastcoreState> {
        let store = Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("in-memory kevy"),
        );
        let mailbox = KevyMailboxStore::new(store);
        Arc::new(FastcoreState::new(mailbox))
    }

    pub(super) fn arr<'a>(tid: &'a str, user: &'a str, unread: bool) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread,
            is_own: !unread,
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn sweep_legacy_admin_keys_clears_legacy_and_keeps_v2() {
        let state = fresh_state();
        let store = state.mailbox.store_ref();
        // Seed the pre-P6 legacy layout + a v2 hash that must survive.
        store.set(b"mailrs:alias:old@x", b"target@x").unwrap();
        store
            .set(b"mailrs:domain:old.example", b"1700000000")
            .unwrap();
        store
            .sadd(b"mailrs:aliases:index", &[b"old@x".as_slice()])
            .unwrap();
        store
            .sadd(b"mailrs:domains:index", &[b"old.example".as_slice()])
            .unwrap();
        store
            .sadd(b"mailrs:accounts:index", &[b"a@x".as_slice()])
            .unwrap();
        state.mailbox.upsert_alias("keep@x", "target@x").unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/admin/maintenance:sweep-legacy-admin-keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        assert!(body.contains("\"legacy_alias_strings\":1"), "{body}");
        assert!(body.contains("\"legacy_domain_strings\":1"), "{body}");
        assert!(body.contains("\"legacy_index_sets\":3"), "{body}");

        // Legacy keys gone; v2 hash intact.
        assert!(store.get(b"mailrs:alias:old@x").unwrap().is_none());
        assert!(store.get(b"mailrs:domain:old.example").unwrap().is_none());
        assert!(store.smembers(b"mailrs:aliases:index").unwrap().is_empty());
        assert_eq!(
            state.mailbox.resolve_alias("keep@x").unwrap().as_deref(),
            Some("target@x")
        );

        // Idempotent: second sweep finds nothing.
        let app2 = build_router(state);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/admin/maintenance:sweep-legacy-admin-keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body2 = body_string(resp2).await;
        assert!(body2.contains("\"legacy_alias_strings\":0"), "{body2}");
        assert!(body2.contains("\"legacy_index_sets\":0"), "{body2}");
    }

    #[tokio::test]
    async fn healthz_reports_kevy_backend() {
        let app = build_router(fresh_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        assert!(body.contains("\"backend\":\"kevy\""), "{body}");
    }

    #[tokio::test]
    async fn unseen_count_after_arrival_is_one() {
        let state = fresh_state();
        state
            .mailbox
            .record_message_arrival(&arr("t1", "u@x.com", true))
            .unwrap();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/users/u@x.com/conversations/unseen-count")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(body_string(resp).await.contains("\"count\":1"));
    }

    #[tokio::test]
    async fn mark_read_drops_from_unseen() {
        let state = fresh_state();
        state
            .mailbox
            .record_message_arrival(&arr("t1", "u@x.com", true))
            .unwrap();
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/threads/t1/read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            state
                .mailbox
                .get_thread("t1")
                .unwrap()
                .unwrap()
                .unread_count,
            0
        );
    }

    #[tokio::test]
    async fn mark_read_on_missing_returns_200_idempotent() {
        // Post 5eb8cc07 mutations are idempotent — a missing thread row
        // returns 200 (noop success) instead of 404 so the UI's optimistic
        // patch doesn't flicker back to unread. Reconciliation happens on
        // the next list refetch.
        let app = build_router(fresh_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/threads/nope/read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn list_conversations_returns_arrivals() {
        let state = fresh_state();
        for i in 0..3 {
            state
                .mailbox
                .record_message_arrival(&MessageArrival {
                    thread_id: &format!("t{i}"),
                    user: "u@x.com",
                    subject: "Subj",
                    senders_csv: "x@y.z",
                    latest_date: i as i64 * 100,
                    latest_preview: "preview",
                    category: "inbox",
                    unread: true,
                    is_own: false,
                })
                .unwrap();
        }
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/conversations:list")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"limit":10}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        // reverse chronological → t2 first
        assert!(body.contains(r#""thread_id":"t2""#));
    }

    /// Smoke every business route — verifies no 404 from a router-
    /// resolution bug. Each route is hit with a request that should
    /// land on the handler; expected statuses are documented inline
    /// (the handler's own 204/404 logic is what we then assert).
    #[tokio::test]
    async fn every_route_resolves_no_404() {
        let state = fresh_state();
        // Seed one thread + one message so the routes have a real
        // target to flip / read.
        state
            .mailbox
            .deliver_message(&arr("t1", "u@x.com", true), "m1", b"{}")
            .unwrap();

        struct Probe {
            method: Method,
            uri: &'static str,
            allowed: &'static [u16],
        }
        let probes: &[Probe] = &[
            // Conversations
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/conversations:list",
                allowed: &[200, 415, 422],
            }, // 415/422 if empty body, 200 with body
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/categories",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/unseen-count",
                allowed: &[200],
            },
            // Thread read
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/threads/t1/messages",
                allowed: &[200],
            },
            // Thread mutations (return 204 on existing tid, 404 on missing)
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/read",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/pin",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unpin",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/star",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unstar",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/archive",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unarchive",
                allowed: &[200],
            },
            Probe {
                method: Method::DELETE,
                uri: "/v1/users/u@x.com/threads/t1",
                allowed: &[200],
            }, // delete after archive may already be gone
            // Probes
            Probe {
                method: Method::GET,
                uri: "/v1/healthz",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/readyz",
                allowed: &[200],
            },
        ];

        for p in probes {
            let app = build_router(state.clone());
            let body = if p.method == Method::POST && p.uri.ends_with(":list") {
                Body::from(r#"{"limit":10}"#)
            } else {
                Body::empty()
            };
            let resp = app
                .oneshot(
                    Request::builder()
                        .method(p.method.clone())
                        .uri(p.uri)
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            let code = resp.status().as_u16();
            assert!(
                p.allowed.contains(&code),
                "{} {} returned {code}, expected {:?}",
                p.method,
                p.uri,
                p.allowed
            );
            assert_ne!(code, 404, "router did not match: {} {}", p.method, p.uri);
        }
    }
}

#[cfg(test)]
mod input_reporting_tests {
    //! A maintenance route must not answer the same thing to "nothing to do"
    //! and "nothing seen".
    //!
    //! `backfill-threading` answered
    //! `{"merged_threads":0,"moved_messages":0,"msgids_indexed":9}` while it
    //! was enumerating a zset nothing writes. Every number was true and the
    //! response was unreadable: the `9` was the only sign the walk was blind,
    //! and it took two failed repair attempts to notice. The counters that
    //! say what was *looked at* are what make a row of zeros legible, and a
    //! comment cannot enforce that they stay.
    use super::tests::{arr, fresh_state};
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    async fn post_json(state: &Arc<FastcoreState>, uri: &str) -> serde_json::Value {
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn state_with_a_mailbox() -> Arc<FastcoreState> {
        let state = fresh_state();
        // Every maintenance route starts from `list_account_addresses`, so a
        // store with threads and no account is one where they all report
        // zero — which is itself the ambiguity under test, and the reason
        // `accounts` is now in the response.
        state
            .mailbox
            .upsert_account(
                "u@x.com",
                r#"{"address":"u@x.com","active":true,"created_at":0}"#,
            )
            .expect("account");
        for tid in ["t1", "t2", "t3"] {
            state
                .mailbox
                .record_message_arrival(&arr(tid, "u@x.com", true))
                .expect("record");
        }
        state
    }

    /// The route the lesson came from. Its zeros must be readable.
    #[tokio::test]
    async fn backfill_threading_says_what_it_enumerated() {
        let empty = post_json(&fresh_state(), "/v1/admin/backfill-threading").await;
        let full = post_json(&state_with_a_mailbox(), "/v1/admin/backfill-threading").await;

        assert_eq!(
            empty["threads_enumerated"], 0,
            "an empty store enumerates nothing"
        );
        assert_ne!(
            full["threads_enumerated"],
            serde_json::json!(0),
            "a populated store must report the threads it walked — this is \
             the field whose absence made `merged_threads: 0` ambiguous"
        );
        assert_ne!(
            empty, full,
            "the two runs must be distinguishable from the response alone"
        );
    }

    /// Same property, stated once per route so a new one cannot quietly
    /// skip it. Each entry is a route whose result fields are all counts of
    /// *work done*, which are zero in both situations.
    #[tokio::test]
    async fn every_work_route_distinguishes_no_input_from_no_work() {
        let routes = [
            "/v1/admin/backfill-threading",
            "/v1/admin/maintenance:backfill-thread-importance",
            "/v1/admin/maintenance:backfill-triage",
        ];
        for uri in routes {
            let empty = post_json(&fresh_state(), uri).await;
            let full = post_json(&state_with_a_mailbox(), uri).await;
            assert_ne!(
                empty, full,
                "{uri} answers identically whether or not there is anything \
                 to look at, so a zero from it cannot be interpreted"
            );
        }
    }
}
