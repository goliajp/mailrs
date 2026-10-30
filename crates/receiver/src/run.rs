//! The standalone receiver process entry point (P6).
//!
//! Assembles a **narrow** `ConnectionContext` — SMTP I/O + anti (greylist /
//! rate / auth via the shared network kevy-server) + antispam pipeline
//! (SPF/DKIM/DMARC + clamav/content) + a spool sink + a cross-process notify
//! publisher. It resolves nothing and stores nothing in spg: recipient
//! resolution / sieve / relay / quota all run later in the core consumer.
//! Accepted mail is written to `{spool_root}/incoming` and a `SpoolDelivered`
//! event is published; the core fetches + delivers it.

use std::sync::Arc;

use mailrs_core::event_bus::EventBus;

use crate::config::ReceiverConfig;
use crate::conn_metrics::ConnectionMetrics;
use crate::kevy_net::KevyNetClient;
use crate::kevy_notify::{
    KevyEventPublisher, KevyTracePublisher, NOTIFY_CHANNEL, process_origin, publish_stats,
};
use crate::listeners::Receiver;
use crate::smtp_session::ConnectionContext;
use crate::smtp_session::spool_sink::MaildirSpoolSink;
use crate::stats::{ReceiverCounters, SharedCounters};
use receiver_metrics::ReceiverMetrics;

mod core_link;
mod greylisting;
mod inbound;
mod receiver_metrics;

/// Build the narrow context and bind the SMTP listeners. They keep
/// accepting until the returned [`Receiver`] is stopped or dropped; a port
/// of 0 binds whatever the system picks, which the `Receiver` reports.
pub async fn start() -> Receiver {
    let cfg = ReceiverConfig::from_env();
    if cfg.kevy_url.is_empty() {
        panic!(
            "MAILRS_KEVY_URL is required for the receiver process — it shares anti state + \
             notify with the core over a network kevy-server"
        );
    }

    tracing::info!(
        hostname = %cfg.hostname,
        spool_root = %cfg.spool_root,
        kevy_url = %cfg.kevy_url,
        "starting mailrs-receiver (split topology)"
    );

    // shared network kevy-server: anti state + the SpoolDelivered notify.
    let kevy_client = Arc::new(KevyNetClient::new(&cfg.kevy_url));

    // anti subsystems, all network-backed (the receiver has no embedded kevy).
    let rate_limiter = inbound::rate_limiter(&cfg, &kevy_client);
    let auth_guard = inbound::auth_guard(&kevy_client);
    let greylisting = greylisting::greylisting(&kevy_client);
    let resolver = inbound::dns_resolver();
    let mail_auth_resolvers = inbound::mail_auth_resolvers(&cfg, resolver.as_ref());

    // The admin's black/white sender lists live in the shared network kevy
    // (admin:greylist:local-lists, written by webapi) and apply with or
    // without greylisting. Reloaded every 60s so an entry added in the
    // admin UI takes effect without a receiver restart.
    let greylist_local = crate::greylist_local::empty();
    crate::greylist_local_sync::spawn_reload_task(greylist_local.clone(), kevy_client.clone(), 60);
    let inbound_pipeline = inbound::pipeline(
        &cfg,
        &kevy_client,
        greylisting,
        greylist_local.clone(),
        resolver.clone(),
        mail_auth_resolvers,
    );

    let tls_state = core_link::tls_state(&cfg);
    let users = core_link::users(&cfg);
    let credential_store = core_link::credential_store(&cfg);
    let recipient_check = core_link::recipient_check(&cfg);
    let runtime = core_link::runtime_settings(&cfg, &kevy_client, greylist_local).await;

    let counters: SharedCounters = Arc::new(ReceiverCounters::default());
    let event_bus = event_bus(&kevy_client, &counters);

    let spool_sink: Arc<dyn crate::smtp_session::SpoolSink> =
        Arc::new(MaildirSpoolSink::new(&cfg.spool_root));

    if cfg.trusted_relays.is_empty() {
        tracing::warn!(
            "MAILRS_TRUSTED_RELAY_IPS is empty: our own mail coming back through this MX is scored like a stranger's"
        );
    } else {
        tracing::info!(relays = ?cfg.trusted_relays, "trusted relays for our own mail");
    }
    let ctx = Arc::new(ConnectionContext {
        hostname: cfg.hostname.clone(),
        tls_state,
        users,
        event_bus,
        metrics: Arc::new(ReceiverMetrics {
            counters: counters.clone(),
        }) as Arc<dyn ConnectionMetrics>,
        rate_limiter,
        runtime,
        org_names: cfg.org_names.clone(),
        org_name_allowed_domains: cfg.org_name_allowed_domains.clone(),
        trusted_relays: cfg.trusted_relays.clone(),
        resolver,
        dnsbl_zones: cfg.dnsbl_zones.clone(),
        dnsbl_enabled: cfg.dnsbl_enabled,
        antispam_enabled: cfg.antispam_enabled,
        smuggle_protection: cfg.smuggle_protection,
        submission_from_check: cfg.submission_from_check,
        auth_guard,
        account_store: None,
        credential_store,
        recipient_check,
        ldap_config: None,
        inbound_pipeline,
        spam_lists_client: Some(kevy_client.clone()),
        spool_sink,
        signins: Default::default(),
    });
    listen_for_signouts(&cfg.kevy_url, ctx.clone());

    crate::listeners::listen(ctx, &cfg).await
}

/// event bus that publishes cross-process events (SpoolDelivered) to the
/// shared kevy-server. The receiver only publishes — the core runs the
/// subscriber bridge.
fn event_bus(kevy_client: &Arc<KevyNetClient>, counters: &SharedCounters) -> EventBus {
    spawn_stats_heartbeat(kevy_client.clone(), counters.clone());

    let publisher = Arc::new(KevyEventPublisher::new(
        kevy_client.clone(),
        NOTIFY_CHANNEL.to_vec(),
        process_origin(),
    ));
    // Second channel, same mechanism: the SMTP session trace. Without
    // this the whole protocol trace stops inside this process — the
    // monitor page in webapi-fc connects, reports "connected", and shows
    // nothing for as long as it is left open.
    let trace_publisher = Arc::new(KevyTracePublisher::new(kevy_client.clone()));
    EventBus::new(1024)
        .with_publisher(publisher)
        .with_trace_publisher(trace_publisher)
}

/// Run the receiver process: [`start`] it and block until a shutdown signal.
pub async fn run() {
    let receiver = start().await;
    tracing::info!("mailrs-receiver listening; awaiting shutdown signal");
    wait_for_shutdown().await;
    receiver.stop();
    tracing::info!("mailrs-receiver shutting down");
}

/// Push the four counters onto the stats channel on a slow tick.
///
/// This is a heartbeat, not a periodic *repair*: there is no stable "nothing to
/// do" state to converge to, because `uptime_secs` differs on every tick
/// by construction. What still applies is that the tick be
/// cheap and bounded — one small PUBLISH to a channel that is a no-op
/// when nobody has subscribed, and nothing written to the durable store.
fn spawn_stats_heartbeat(client: Arc<KevyNetClient>, counters: SharedCounters) {
    const TICK: std::time::Duration = std::time::Duration::from_secs(10);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        loop {
            ticker.tick().await;
            publish_stats(&client, &counters.snapshot());
        }
    });
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm");
    tokio::select! {
        r = tokio::signal::ctrl_c() => { let _ = r; }
        _ = sigterm.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Close submission connections when their account's sign-ins end, and,
/// each time the subscription is (re)made, those of accounts cleared while
/// it was down.
fn listen_for_signouts(kevy_url: &str, ctx: Arc<ConnectionContext>) {
    let ended = ctx.signins.clone();
    let runtime = tokio::runtime::Handle::current();
    mailrs_core_sidestate::keys::signout::listen(
        kevy_url.to_string(),
        move |account| {
            ended.end(account);
        },
        move || {
            let ctx = ctx.clone();
            runtime.spawn(async move { end_cleared(&ctx).await });
        },
    );
}

/// Close the submission connections of every account signed in here whose
/// password has been cleared.
pub(crate) async fn end_cleared(ctx: &ConnectionContext) {
    let Some(credentials) = &ctx.credential_store else {
        return;
    };
    for account in ctx.signins.accounts() {
        if matches!(credentials.password_cleared(&account).await, Ok(true)) {
            let closed = ctx.signins.end(&account);
            tracing::info!(
                account = %mailrs_auth_guard::login::loggable_account(&account),
                closed,
                "closed submission connections of an account signed out while unheard"
            );
        }
    }
}
