//! Opening the kevy store and building the process state on top of it.

use std::sync::Arc;

use crate::*;

/// Open the store, declare what the engine maintains, and build the state.
pub(super) fn open_state(kevy_dir: &str) -> Arc<FastcoreState> {
    let cfg = kevy_config(kevy_dir);
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

    // Publish who holds an account here, for the receiver — which has
    // no account store and has to know, or a stranger wearing one of
    // our own people's names arrives unremarked
    // (`impersonates_one_of_us`). One writer, one source: the
    // alternative is a second copy in a variable, and that
    // arrangement already failed once when `MAILRS_ORG_NAMES` was set
    // on the receiver and not here.
    publish_account_names(&mailbox);
    let alias_store = alias_store(&mailbox);
    Arc::new(
        FastcoreState::new_with_alias_store(mailbox, alias_store).with_boot_intact(boot_intact),
    )
}

/// The store configuration, from the data directory and the environment.
fn kevy_config(kevy_dir: &str) -> Config {
    // v2 Stage B.8: enable the kevy 3.17 change feed so IMAP IDLE (and
    // future JMAP push, WS bridges) can subscribe via changes_since
    // instead of the in-memory tokio broadcast channel. The feed is
    // durable across restarts (offset resumes) and buffers writes so
    // a slow consumer doesn't lose events. 16 MiB buffer ≈ 250K
    // change frames (~64 B each) — plenty for a per-user IDLE
    // consumer under normal load.
    let mut cfg = Config::default()
        .with_persist(kevy_dir)
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
    cfg
}

/// The alias store this process reads and writes, chosen by the environment.
fn alias_store(mailbox: &KevyMailboxStore) -> Arc<dyn AliasStore> {
    // Alias-store backend selector — RFC 20260705 Step 2.
    // Default (`embed` / unset): historical fastcore-owned alias table
    // in the local kevy AOF. Cutover flip: `MAILRS_ALIAS_STORE_BACKEND=network`
    // + `MAILRS_KEVY_URL=…` moves the source of truth into the shared
    // network kevy so pg-core / monolith read the same rows during
    // stack switches — no per-cutover dump/load needed.
    let alias_backend =
        std::env::var("MAILRS_ALIAS_STORE_BACKEND").unwrap_or_else(|_| "embed".into());
    match alias_backend.as_str() {
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
/// Publish every account's display name to the shared kevy.
///
/// Best-effort and at boot only: accounts change rarely, and a
/// receiver that reads nothing delivers rather than holds.
fn publish_account_names(mailbox: &KevyMailboxStore) {
    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::warn!(
            "no network kevy — account display names unpublished, so the receiver cannot \
             see somebody wearing one of our own people's names"
        );
        return;
    };
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        return;
    };
    let names: Vec<String> = mailbox
        .list_account_addresses()
        .unwrap_or_default()
        .iter()
        .filter_map(|a| mailbox.get_account_blob(a).ok().flatten())
        .filter_map(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
        .filter_map(|v| {
            v.get("display_name")
                .and_then(|d| d.as_str())
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string)
        })
        .collect();
    tracing::info!(count = names.len(), "published account display names");
    mailrs_core_sidestate::families::account_names::publish(&mut conn, &names);
}

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
