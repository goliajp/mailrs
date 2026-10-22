//! Greylisting and the remote whitelist that exempts known senders from it.

use std::sync::Arc;

use mailrs_shield::greylist::GreylistDb;

use crate::greylist_sync::GreylistListsHandle;
use crate::inbound::kevy_backends::KevyServerGreylistBackend;
use crate::kevy_net::KevyNetClient;

/// The greylist store, `None` when greylisting is off, and the remote
/// whitelist, synced only when it is on.
pub(super) fn greylisting(
    kevy_client: &Arc<KevyNetClient>,
) -> (Option<Arc<GreylistDb>>, GreylistListsHandle) {
    // Greylisting is off unless `MAILRS_GREYLIST_ENABLED` asks for it.
    //
    // It defers every first delivery by construction — `shield::greylist`
    // answers `Defer` for an unseen (ip, from, to) triplet before any delay
    // is consulted, so no amount of tuning removes the first failure. A
    // sender that retries gets in late; a sender that does not retry, or
    // that retries from a different address in a rotating pool, never gets
    // in at all and nothing on either side says so.
    //
    // Measured on production 2026-08-14, over the ~36 days the triplets
    // live: 504 distinct envelope senders had been deferred, 269 of them
    // eventually arrived, and **235 never did** — among them GMO Aozora
    // Bank, PyPI and other service notifications, and a NuGet sign-up
    // confirmation that was attempted four times inside seven minutes from
    // four different 40.93.x.x addresses, each attempt a fresh triplet and
    // each one deferred. Delivery and receipt outrank protection here, and
    // the other stages (rate limit, PTR, DNSBL, SPF/DKIM/DMARC,
    // sender-trust, Bayesian content scoring) do not have this failure
    // mode.
    let greylist_enabled = greylisting_requested(std::env::var("MAILRS_GREYLIST_ENABLED").ok());
    let greylist_db = if greylist_enabled {
        Some(Arc::new(GreylistDb::with_backend(Arc::new(
            KevyServerGreylistBackend::new(kevy_client.clone()),
        ))))
    } else {
        tracing::info!(
            "greylisting disabled (set MAILRS_GREYLIST_ENABLED=1 to defer first deliveries)"
        );
        None
    };

    // Whitelist wire-up. Split-topology receiver was shipping with
    // `empty()` here, which meant *every* new sender got greylisted
    // — Gmail / Outlook / etc. all deferred on first attempt even
    // though the whitelist code already knew to skip them. Fix: kick
    // off the same remote-sync task the monolith used, driven by
    // `MAILRS_GREYLIST_WHITELIST_URL` (with a sensible default so
    // operators don't need to set it explicitly).
    //
    // Gated on the same switch as the stage: a whitelist is only meaningful
    // when something is being deferred, and an hourly HTTP fetch that
    // nothing reads is a periodic task with no consumer.
    let greylist_lists = crate::greylist_sync::empty();
    if greylist_enabled {
        let url = std::env::var("MAILRS_GREYLIST_WHITELIST_URL").unwrap_or_else(|_| {
            "https://raw.githubusercontent.com/goliajp/mailrs/develop/assets/greylist-whitelist.txt".to_string()
        });
        let interval = std::env::var("MAILRS_GREYLIST_WHITELIST_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600u64);
        let cache = std::env::var("MAILRS_GREYLIST_CACHE_PATH")
            .unwrap_or_else(|_| "/data/.greylist-whitelist.cache".to_string());
        tracing::info!(%url, interval, %cache, "spawning greylist whitelist sync");
        crate::greylist_sync::spawn_sync_task(greylist_lists.clone(), url, interval, Some(cache));
    }
    (greylist_db, greylist_lists)
}

/// Whether the operator has asked for greylisting.
///
/// Absent means **off**. Greylisting defers every first delivery by
/// construction, and on this deployment that lost 235 of 504 deferred
/// senders outright (see `greylisting`); switching it on is a deliberate
/// trade of delivery for filtering, so it takes an explicit value.
fn greylisting_requested(var: Option<String>) -> bool {
    matches!(var.as_deref(), Some("1") | Some("true"))
}

#[cfg(test)]
mod tests {
    use super::greylisting_requested;

    /// The default is the whole point: a deployment that says nothing must
    /// not defer first deliveries.
    #[test]
    fn absent_means_off() {
        assert!(!greylisting_requested(None));
    }

    #[test]
    fn only_an_explicit_yes_turns_it_on() {
        assert!(greylisting_requested(Some("1".into())));
        assert!(greylisting_requested(Some("true".into())));

        // Anything else is off rather than on, so a typo or a leftover
        // `MAILRS_GREYLIST_ENABLED=0` cannot re-enable deferral by accident.
        for v in ["0", "false", "no", "", "yes", "on", "True", " 1"] {
            assert!(
                !greylisting_requested(Some(v.into())),
                "{v:?} must not enable greylisting"
            );
        }
    }
}
