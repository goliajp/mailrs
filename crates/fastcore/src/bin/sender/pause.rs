//! Holding an account's mail when it sends far past its hourly limit or
//! keeps failing; the state is `core_sidestate::families::outbound::pause`.
//!
//! A job is counted when it is claimed, on its first attempt: a job is one
//! recipient, so the recipient past the limit is held before any of it
//! leaves. Forwarded mail (a job with an original sender) is the sending
//! of whoever wrote to the account, not the account's, and security
//! alerts must leave whatever their account's state; neither is counted
//! or held.

use std::collections::HashMap;

use mailrs_auth_guard::alerts::{Alert, Kind};
use mailrs_core_sidestate::families::outbound::{SECURITY_ALERT_KIND, pause};
use tokio::task::spawn_blocking;

use super::{Cfg, kevy, now_secs};

/// The hourly ceilings: the defaults, and per account where an override
/// names it. Always finite.
#[derive(Debug, Clone)]
pub(super) struct PauseLimits {
    default: pause::Limits,
    overrides: HashMap<String, pause::Limits>,
}

impl PauseLimits {
    /// From `MAILRS_OUTBOUND_PAUSE_PER_HOUR`, `..._FAILS_PER_HOUR` and
    /// `..._OVERRIDES`; a value that does not parse stops the process.
    pub(super) fn from_env() -> Result<Self, String> {
        let number = |name: &str, default: u64| match std::env::var(name) {
            Err(_) => Ok(default),
            Ok(v) => v
                .trim()
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or(format!("{name}={v:?}: expected a positive number")),
        };
        let default = pause::Limits {
            per_hour: number(
                "MAILRS_OUTBOUND_PAUSE_PER_HOUR",
                pause::Limits::DEFAULT.per_hour,
            )?,
            fails_per_hour: number(
                "MAILRS_OUTBOUND_PAUSE_FAILS_PER_HOUR",
                pause::Limits::DEFAULT.fails_per_hour,
            )?,
        };
        let raw = std::env::var("MAILRS_OUTBOUND_PAUSE_OVERRIDES").unwrap_or_default();
        let overrides = pause::parse_overrides(&raw)
            .map_err(|e| format!("MAILRS_OUTBOUND_PAUSE_OVERRIDES: {e}"))?
            .into_iter()
            .collect();
        Ok(Self { default, overrides })
    }

    fn for_account(&self, address: &str) -> pause::Limits {
        self.overrides
            .get(&address.to_ascii_lowercase())
            .copied()
            .unwrap_or(self.default)
    }
}

/// The account a job counts against, or `None` for one never counted.
fn counted_account(envelope: &serde_json::Value, sender: &str) -> Option<String> {
    let kind = envelope.get("kind").and_then(|v| v.as_str());
    let forwarded = envelope
        .get("original_sender")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty());
    let address = sender.trim_matches(['<', '>']).to_ascii_lowercase();
    if kind == Some(SECURITY_ALERT_KIND) || forwarded || address.is_empty() {
        return None;
    }
    Some(address)
}

/// Count the job and hold it when its account is paused or goes over its
/// hour now. True when it was held: the caller sends nothing.
pub(super) async fn held(
    cfg: &Cfg,
    id: &str,
    envelope: &serde_json::Value,
    sender: &str,
    first_attempt: bool,
) -> bool {
    let Some(account) = counted_account(envelope, sender) else {
        return false;
    };
    let limits = cfg.pause_limits.for_account(&account);
    let created_at = envelope
        .get("created_at")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let (url, id) = (cfg.kevy_url.clone(), id.to_string());
    let acct = account.clone();
    let res = spawn_blocking(move || {
        let mut c = kevy(&url)?;
        decide(
            &mut c,
            &acct,
            &id,
            created_at,
            first_attempt,
            limits,
            now_secs(),
        )
    })
    .await;
    match res {
        Ok(Ok((held, newly))) => {
            if let Some(reason) = newly {
                announce(cfg, &account, &reason, "outbound.pause").await;
            }
            if held {
                tracing::warn!(%account, "outbound paused: job held");
            }
            held
        }
        // the queue's own store is the same server: a job that cannot be
        // counted cannot be delivered either, and is retried as usual
        other => {
            tracing::warn!(?other, %account, "pause check failed; sending");
            false
        }
    }
}

/// Whether `account`'s job `id` is held, and the reason when this job is
/// the one that paused it.
fn decide(
    c: &mut kevy_client::Connection,
    account: &str,
    id: &str,
    created_at: i64,
    first_attempt: bool,
    limits: pause::Limits,
    now: i64,
) -> std::io::Result<(bool, Option<String>)> {
    let mut newly = None;
    if !pause::is_paused(c, account)? {
        if !first_attempt {
            return Ok((false, None));
        }
        let n = pause::count_send(c, account, now)?;
        if n <= limits.per_hour {
            return Ok((false, None));
        }
        let reason = format!(
            "{n} recipients in an hour, over its limit of {}",
            limits.per_hour
        );
        if pause::pause(c, account, &reason, now)? {
            newly = Some(reason);
        }
    }
    pause::hold(c, account, id, created_at)?;
    Ok((true, newly))
}

/// Count a permanent failure of `sender`'s job, pausing the account when
/// its hour's failures pass the limit.
pub(super) async fn note_permanent_failure(cfg: &Cfg, envelope: &serde_json::Value, sender: &str) {
    let Some(account) = counted_account(envelope, sender) else {
        return;
    };
    let limits = cfg.pause_limits.for_account(&account);
    let (url, acct) = (cfg.kevy_url.clone(), account.clone());
    let res = spawn_blocking(move || -> std::io::Result<Option<String>> {
        let mut c = kevy(&url)?;
        let now = now_secs();
        let n = pause::count_failure(&mut c, &acct, now)?;
        if n <= limits.fails_per_hour {
            return Ok(None);
        }
        let reason = format!(
            "{n} permanent failures in an hour, over its limit of {}",
            limits.fails_per_hour
        );
        Ok(pause::pause(&mut c, &acct, &reason, now)?.then_some(reason))
    })
    .await;
    match res {
        Ok(Ok(Some(reason))) => announce(cfg, &account, &reason, "outbound.pause").await,
        Ok(Ok(None)) => {}
        other => tracing::warn!(?other, %account, "failure count not recorded"),
    }
}

/// Alert the operators and write the audit fact for `account`.
async fn announce(cfg: &Cfg, account: &str, detail: &str, action: &'static str) {
    let alert = Alert {
        kind: Kind::OutboundPaused,
        subject: account.to_string(),
        at: now_secs().max(0) as u64,
        detail: detail.to_string(),
    };
    if let Err(e) = mailrs_auth_guard::kevy::guard_for(Some(&cfg.kevy_url))
        .raise(alert)
        .await
    {
        tracing::error!(error = %e, %account, "outbound pause alert not queued");
    }
    let (url, account, detail) = (
        cfg.kevy_url.clone(),
        account.to_string(),
        detail.to_string(),
    );
    let res = spawn_blocking(move || {
        mailrs_core_sidestate::families::audit::append(
            &mut kevy(&url)?,
            "system",
            action,
            &account,
            &detail,
            now_secs(),
        )
    })
    .await;
    if !matches!(res, Ok(Ok(()))) {
        tracing::error!(?res, "outbound pause not audited");
    }
}

/// Once per sweep: repeat the alert of every account paused a day since
/// it was last announced, and let go of held mail older than the queue
/// lifetime, with a bounce to its sender. Nothing paused is two reads.
pub(super) async fn sweep(cfg: &Cfg) {
    let url = cfg.kevy_url.clone();
    let lifetime = cfg.max_queue_lifetime_secs;
    let res = spawn_blocking(move || -> std::io::Result<Vec<Swept>> {
        let mut c = kevy(&url)?;
        let now = now_secs();
        let mut out = Vec::new();
        for p in pause::paused(&mut c)? {
            let due =
                pause::alerted_at(&mut c, &p.address)?.is_none_or(|at| now - at >= REPEAT_SECS);
            if due {
                pause::set_alerted_at(&mut c, &p.address, now)?;
            }
            let mut expired = Vec::new();
            for id in pause::held_ids(&mut c, &p.address, now - lifetime)? {
                if let Some(blob) = pause::drop_held(&mut c, &p.address, &id)? {
                    expired.push(blob);
                }
            }
            out.push(Swept {
                paused: p,
                repeat: due,
                expired,
            });
        }
        Ok(out)
    })
    .await;
    let swept = match res {
        Ok(Ok(s)) => s,
        other => {
            tracing::warn!(?other, "outbound pause sweep failed");
            return;
        }
    };
    for s in swept {
        if s.repeat {
            let detail = format!("still paused since {}: {}", s.paused.since, s.paused.reason);
            announce(cfg, &s.paused.address, &detail, "outbound.pause_repeat").await;
        }
        if !s.expired.is_empty() {
            bounce_expired(cfg, &s.paused.address, &s.expired).await;
        }
    }
}

const REPEAT_SECS: i64 = 24 * 3600;

#[derive(Debug)]
struct Swept {
    paused: pause::Paused,
    repeat: bool,
    expired: Vec<Vec<u8>>,
}

/// Bounce each expired held job to its sender and audit the count.
async fn bounce_expired(cfg: &Cfg, account: &str, blobs: &[Vec<u8>]) {
    for blob in blobs {
        let Ok(env) = serde_json::from_slice::<serde_json::Value>(blob) else {
            continue;
        };
        let field = |k: &str| {
            env.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let message = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(field("message_data_b64"))
                .unwrap_or_default()
        };
        super::enqueue_bounce_dsn(
            cfg,
            &field("sender"),
            &field("recipient"),
            "not sent: outbound mail from this account was paused and nobody released it",
            &message,
        )
        .await;
    }
    let detail = format!(
        "{} held messages discarded at the end of the queue lifetime",
        blobs.len()
    );
    announce(cfg, account, &detail, "outbound.pause_expired").await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_and_forwards_are_not_counted() {
        let plain = serde_json::json!({ "sender": "a@golia.jp" });
        assert_eq!(
            counted_account(&plain, "<A@golia.jp>").as_deref(),
            Some("a@golia.jp")
        );
        let alert = serde_json::json!({ "kind": SECURITY_ALERT_KIND });
        assert_eq!(counted_account(&alert, "noreply@golia.jp"), None);
        let forward = serde_json::json!({ "original_sender": "x@example.org" });
        assert_eq!(counted_account(&forward, "SRS0=a@golia.jp"), None);
        // a kind nobody wrote, or any other value, counts as ordinary mail
        let other = serde_json::json!({ "kind": "Security_Alert" });
        assert!(counted_account(&other, "a@golia.jp").is_some());
    }

    fn job(
        c: &mut kevy_client::Connection,
        sender: &str,
        alert: bool,
    ) -> (String, serde_json::Value) {
        use mailrs_core_sidestate::families::outbound as ob;
        let fresh = ob::FreshPending {
            sender,
            recipient: "someone@example.org",
            message_data_base64: "aGk=",
            scheduled_at: None,
            original_sender: None,
            send_id: None,
        };
        let id = match alert {
            true => ob::write_security_alert_pending(c, &fresh, 1_000),
            false => ob::write_fresh_pending(c, &fresh, 1_000),
        }
        .unwrap();
        let blob = c
            .hget(ob::job_key(id).as_bytes(), b"blob")
            .unwrap()
            .unwrap();
        (id.to_string(), serde_json::from_slice(&blob).unwrap())
    }

    /// One job through the sender's check, as `held` runs it.
    fn claim(
        c: &mut kevy_client::Connection,
        id: &str,
        env: &serde_json::Value,
        limits: pause::Limits,
    ) -> bool {
        let sender = env["sender"].as_str().unwrap();
        let Some(account) = counted_account(env, sender) else {
            return false;
        };
        decide(c, &account, id, 1_000, true, limits, 7_200)
            .unwrap()
            .0
    }

    /// Against a kevy-server: writing a job pipelines, which the in-process
    /// store does not.
    #[tokio::test]
    async fn the_alert_account_paused_by_its_own_mail_still_sends_alerts() {
        let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        let limits = pause::Limits {
            per_hour: 2,
            fails_per_hour: 30,
        };
        let alerts = "noreply@golia.jp";
        for n in 1..=3 {
            let (id, env) = job(&mut c, alerts, false);
            assert_eq!(claim(&mut c, &id, &env, limits), n > 2, "ordinary job {n}");
        }
        assert!(pause::is_paused(&mut c, alerts).unwrap());
        // the alert announcing that pause is from the same account, and goes
        let (id, env) = job(&mut c, alerts, true);
        assert!(
            !claim(&mut c, &id, &env, limits),
            "the OutboundPaused alert was held"
        );
        assert_eq!(pause::held_ids(&mut c, alerts, i64::MAX).unwrap().len(), 1);
    }
}
