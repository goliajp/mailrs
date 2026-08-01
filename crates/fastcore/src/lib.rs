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
mod calendar_sync;
pub mod dmarc_ingest;
pub mod fbl;
mod idle_backoff;
mod imap;
mod importance;
mod junk_ttl;
pub mod live_sync;
mod maildir_scan;
mod maintenance;
mod managesieve;
mod pop3;
mod routes;
use maildir_scan::*;
use maintenance::*;
pub mod sender_sts;
mod sieve_apply;
mod spool_drain;
pub mod tlsrpt;
pub mod tlsrpt_ingest;
mod webhook_delivery;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
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
                        let _ = state.mailbox.upsert_user_message(
                            user,
                            &s.thread_id,
                            &w.message_id,
                            w.internal_date,
                            &payload,
                            &mailrs_mailbox_kevy::UserMessageFacts {
                                blob_ref: &w.blob_ref,
                                uid: w.uid,
                                flags: w.flags,
                                modseq: w.modseq,
                            },
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
                    let _ = state.mailbox.upsert_user_message(
                        user,
                        &s.thread_id,
                        &w.message_id,
                        w.internal_date,
                        &payload,
                        &mailrs_mailbox_kevy::UserMessageFacts {
                            blob_ref: &w.blob_ref,
                            uid: w.uid,
                            flags: w.flags,
                            modseq: w.modseq,
                        },
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
    // Webhook subscriptions filtered to this sender / conversation. The
    // monolith enqueued here off its event bus; this lane had no
    // subscriber, so a user's webhook never fired at all.
    enqueue_webhooks_for_arrival(state, addr, &root, &from, &subject);
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
            // The shared blob plus this user's own row: their maildir
            // filename, their uid, their flags. A thread can have several
            // owners and each has a different file on disk, so a single
            // `blob_ref` on the shared blob is one owner's — 74 messages on
            // production were served to a user the row did not name. See
            // `.claude/rfcs/20260731-per-user-message-projection.md`.
            if let Err(e) = state.mailbox.upsert_user_message(
                addr,
                &root,
                &message_id,
                date,
                &payload,
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref,
                    uid: wire.uid,
                    flags: wire.flags,
                    modseq: wire.modseq,
                },
            ) {
                tracing::warn!(error = %e, %addr, %root, "drain ingest: upsert_user_message failed");
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
            // Ops endpoint — give pre-existing webhook subscriptions the
            // owner entry their delete path now needs. Idempotent.
            .route(
                "/v1/admin/maintenance:backfill-webhook-owners",
                post(backfill_webhook_owners_route),
            )
            // Ops endpoint — fold the retired `agent:webhooks:{user}`
            // namespace into the one both surfaces now read. Idempotent.
            .route(
                "/v1/admin/maintenance:migrate-agent-webhooks",
                post(migrate_agent_webhooks_route),
            )
            // Stage 2 of the per-user message projection: give every user
            // their own row for the messages they actually have.
            .route(
                "/v1/admin/maintenance:backfill-user-messages",
                post(backfill_user_messages_route),
            )
            // One-shot: the per-user message index's first key spelling sat
            // inside the prefix `all_thread_ids_for_user` enumerates.
            .route(
                "/v1/admin/maintenance:drop-stray-usermsg-keys",
                post(drop_stray_usermsg_keys_route),
            )
            // Stage 3: compare the shared index against the per-user one
            // before anything reads the latter.
            .route(
                "/v1/admin/maintenance:threadrow-shadow",
                post(threadrow_shadow_route),
            )
            .route(
                "/v1/admin/maintenance:strip-shared-per-user-fields",
                post(strip_shared_per_user_fields_route),
            )
            .route(
                "/v1/admin/maintenance:usermsg-shadow",
                post(usermsg_shadow_route),
            )
            // Ops endpoint — where mail forging one of our own domains
            // actually ended up.
            .route(
                "/v1/admin/maintenance:spoof-landing",
                post(spoof_landing_route),
            )
            // Ops endpoint — remove thread rows that open onto nothing.
            .route(
                "/v1/admin/maintenance:drop-empty-threads",
                post(drop_empty_threads_route),
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
    // An error here reads as an empty mailbox, which is the one answer
    // the caller cannot tell from a real one — so say it happened. The
    // dispatcher returns Err when a query names a column its index does
    // not store, and that is a wiring mistake, not an empty page.
    let (rows, _total) = state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, limit)
        .unwrap_or_else(|e| {
            tracing::warn!(%user, error = %e, "conversation list failed; serving empty");
            (Vec::new(), 0)
        });

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
    // The searcher's own copy of each hit. The shared hash has no user
    // segment, so reading it here handed one owner's counters, flags and
    // category to the other — the same defect the list had.
    let items = tids
        .into_iter()
        .filter_map(|tid| {
            state
                .mailbox
                .get_thread_for_user(&user, &tid)
                .ok()
                .flatten()
        })
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
        .get_thread_for_user(&user, &thread_id)
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
        .get_thread_for_user(&user, &thread_id)
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
    // Each caller's own copy. Reading the shared hash here served one
    // owner's counters and flags to the other, and this is the endpoint
    // the client uses to refresh a conversation it already has open.
    let items = req
        .thread_ids
        .iter()
        .filter_map(|tid| {
            state
                .mailbox
                .get_thread_for_user(&user, tid)
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
    // The sent copy is this user's own: its maildir file is in their
    // mailbox and its uid is theirs. Parsed back out of the payload so the
    // per-user row records what was actually written.
    let sent_wire: Option<mailrs_core_api::method::message::MessageWire> =
        serde_json::from_str(&payload).ok();
    let sent_facts = sent_wire
        .as_ref()
        .map(|w| mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: &w.blob_ref,
            uid: w.uid,
            flags: w.flags,
            modseq: w.modseq,
        });
    let fallback = mailrs_mailbox_kevy::UserMessageFacts {
        blob_ref: "",
        uid: 0,
        flags: 0,
        modseq: 0,
    };
    if let Err(e) = state.mailbox.upsert_user_message(
        user.as_str(),
        &thread_id,
        &req.message_id,
        req.latest_date,
        payload.as_bytes(),
        sent_facts.as_ref().unwrap_or(&fallback),
    ) {
        tracing::error!(err = %e, %user, %thread_id, "upsert_user_message failed");
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

/// Queue a delivery for every subscription that admits this message.
///
/// Best-effort by design: a webhook that cannot be queued must not fail the
/// delivery of the mail itself. Failures are logged rather than swallowed,
/// which is the difference between this and the class of silence the
/// 2026-07-30 audit was about.
fn enqueue_webhooks_for_arrival(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    sender: &str,
    subject: &str,
) {
    use mailrs_core_sidestate::families::{webhook_outbox, webhooks};

    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let subs = match webhooks::matching(&mut conn, user, sender, thread_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(err = %e, %user, "webhook: could not read subscriptions");
            return;
        }
    };
    if subs.is_empty() {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let timestamp = chrono::DateTime::from_timestamp(now, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    let payload =
        webhooks::new_message_payload(user, thread_id, sender, subject, "", &timestamp).to_string();
    for sub in subs {
        match webhook_outbox::enqueue(&mut conn, sub.id, user, &payload, now) {
            Ok(id) => tracing::info!(entry = id, subscription = sub.id, "webhook queued"),
            Err(e) => tracing::warn!(err = %e, subscription = sub.id, "webhook: enqueue failed"),
        }
    }
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
    if let Err(e) = state.mailbox.upsert_user_message(
        user.as_str(),
        &wire.thread_id,
        &wire.message_id,
        wire.date,
        &json,
        &mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: &wire.blob_ref,
            uid: wire.uid,
            flags: wire.flags,
            modseq: wire.modseq,
        },
    ) {
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
mod spoof_landing_tests {
    use super::from_header_domains;

    /// The domain decides whether a DMARC failure is somebody forging one
    /// of our users or just a stranger with a broken setup, so reading it
    /// off a `From:` line is the whole judgement this route makes.
    #[test]
    fn it_reads_the_domain_out_of_the_ordinary_forms() {
        assert_eq!(
            from_header_domains("From: Netflix <takagi@golia.jp>"),
            ["golia.jp"]
        );
        assert_eq!(from_header_domains("From: takagi@golia.jp"), ["golia.jp"]);
        assert_eq!(from_header_domains("from: A B <x@GOLIA.JP>"), ["golia.jp"]);
    }

    /// A display name may itself contain an `@`, which is exactly what a
    /// sender trying to look like one of ours would put there. Reading the
    /// first `@` takes the domain from the quoted part and concludes the
    /// message forged nothing.
    #[test]
    fn a_display_name_containing_an_at_does_not_win() {
        assert_eq!(
            from_header_domains("From: \"billing@paypal.com\" <attacker@evil.example>"),
            ["evil.example"]
        );
    }

    /// The case that caught the first version: it read the *last* `@` and
    /// answered `other.com`, so a header claiming one of ours alongside a
    /// stranger counted as not ours at all. Both are claimed, so both are
    /// returned and the caller checks for any hosted one.
    #[test]
    fn every_address_in_the_header_is_returned() {
        assert_eq!(
            from_header_domains("From: a@golia.jp, b@other.com"),
            ["golia.jp", "other.com"]
        );
        assert_eq!(
            from_header_domains("From: A <a@other.com>, B <b@golia.jp>"),
            ["other.com", "golia.jp"]
        );
    }

    /// A trailing dot is the same domain (RFC 1034 root form).
    #[test]
    fn it_drops_the_root_dot_and_trailing_noise() {
        assert_eq!(from_header_domains("From: <a@golia.jp.>"), ["golia.jp"]);
        assert_eq!(from_header_domains("From: a@golia.jp\r"), ["golia.jp"]);
    }

    #[test]
    fn a_header_with_no_usable_domain_yields_nothing() {
        assert!(from_header_domains("From: not-an-address").is_empty());
        assert!(from_header_domains("From: a@").is_empty());
        assert!(from_header_domains("From:").is_empty());
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
            .deliver_message(
                &arr("t1", "u@x.com", true),
                "m1",
                b"{}",
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref: "1785000000.M1P1.host",
                    uid: 1,
                    flags: 0,
                    modseq: 1,
                },
            )
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
