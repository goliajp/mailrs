//! webapi watching the ops daemon: once a minute it reads the daemon's
//! state, and when the last completed cycle is older than
//! `MAILRS_OPS_SILENT_SECS` (600 by default), or the daemon does not answer,
//! it mails "the ops daemon is silent" to the operator addresses through
//! mailrs's own path, once, and "it is back" when cycles resume.
//!
//! This is the in-product watch; an external dead-man's switch
//! (`MAILRS_OPS_HEARTBEAT_URL` on the daemon) is the additional one that
//! also sees this host go down.
//!
//! Off unless `MAILRS_OPS_URL`, `MAILRS_OPS_ADMIN_SECRET`,
//! `MAILRS_OPS_ALERT_FROM` and `MAILRS_OPS_OPERATOR_ADDRESSES` are all set.

use std::time::Duration;

use mailrs_core_api::method::ops_alert::{OpsAlertWire, OpsOperators};

const EVERY: Duration = Duration::from_secs(60);

/// What the last look found, for the admin view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    Cycling,
    /// Silent since this unix second.
    Silent(i64),
}

/// Whether the daemon counts as silent: no answer, or its last completed
/// cycle older than `limit` seconds.
pub fn silent(state: Option<&serde_json::Value>, now: i64, limit: i64) -> bool {
    let last = state.and_then(|s| s["last_cycle"]["started_at"].as_i64());
    last.is_none_or(|t| now - t > limit)
}

/// The mail a change of state sends; `None` when nothing changed.
pub fn transition(before: Seen, silent_now: bool, now: i64) -> Option<(Seen, OpsAlertWire)> {
    match (before, silent_now) {
        (Seen::Cycling, true) => Some((
            Seen::Silent(now),
            OpsAlertWire {
                id: format!("webapi-watch-{now}"),
                subject: "mailrs: the ops daemon is silent / 运维守护进程没有动静".into(),
                text: "webapi cannot see the ops daemon completing its checks. Until it does, \
                       no alert from it reaches anyone. Check the ops container and its log.\r\n\r\n\
                       webapi 看不到运维守护进程完成检查。在它恢复之前，它的告警发不到任何人。\
                       请查看 ops 容器和它的日志。\r\n"
                    .into(),
            },
        )),
        (Seen::Silent(since), false) => Some((
            Seen::Cycling,
            OpsAlertWire {
                id: format!("webapi-watch-{now}"),
                subject: "mailrs: the ops daemon is checking again / 运维守护进程已恢复".into(),
                text: format!(
                    "The ops daemon completes its checks again; it was silent for {}s.\r\n\
                     运维守护进程恢复了检查；之前没有动静 {} 秒。\r\n",
                    now - since,
                    now - since
                ),
            },
        )),
        _ => None,
    }
}

pub fn spawn() {
    let var = |n: &str| std::env::var(n).ok().filter(|v| !v.trim().is_empty());
    let (Some(url), Some(secret), Some(ops)) = (
        var("MAILRS_OPS_URL"),
        var("MAILRS_OPS_ADMIN_SECRET"),
        OpsOperators::from_env(),
    ) else {
        tracing::info!(
            "ops daemon watch off: MAILRS_OPS_URL, MAILRS_OPS_ADMIN_SECRET or the operator addresses are not set"
        );
        return;
    };
    let limit = crate::handlers::admin_ops_daemon::silent_after_secs();
    let state_url = format!("{}/v1/state", url.trim_end_matches('/'));
    tokio::spawn(async move {
        let http = reqwest::Client::new();
        let mut seen = Seen::Cycling;
        loop {
            tokio::time::sleep(EVERY).await;
            let state = http
                .get(&state_url)
                .header("x-mailrs-ops-admin", &secret)
                .header("x-mailrs-ops-actor", "webapi-watch")
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .ok()
                .filter(|r| r.status().is_success());
            let body = match state {
                Some(r) => r.json::<serde_json::Value>().await.ok(),
                None => None,
            };
            let now = crate::handlers::complete::now_secs();
            let Some((next, alert)) = transition(seen, silent(body.as_ref(), now, limit), now)
            else {
                continue;
            };
            let queued = crate::handlers::kevy_util::with_kevy(|c| {
                mailrs_core_sidestate::families::ops_alerts::submit(c, &ops, &alert, now)
            });
            match queued {
                Ok(n) => {
                    tracing::warn!(state = ?next, mailed = n, "ops daemon watch: state changed");
                    seen = next;
                }
                // tried again on the next look
                Err(_) => tracing::error!("ops daemon watch: the notice could not be queued"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_no_answer_or_an_old_cycle() {
        let state = |t: i64| serde_json::json!({"last_cycle": {"started_at": t}});
        assert!(silent(None, 1000, 600));
        assert!(silent(
            Some(&serde_json::json!({"last_cycle": null})),
            1000,
            600
        ));
        assert!(!silent(Some(&state(500)), 1000, 600));
        assert!(silent(Some(&state(399)), 1000, 600));
    }

    #[test]
    fn one_mail_when_it_goes_silent_and_one_when_it_is_back() {
        let (s, a) = transition(Seen::Cycling, true, 1000).unwrap();
        assert_eq!(s, Seen::Silent(1000));
        assert!(a.subject.contains("silent"));
        assert!(
            transition(s, true, 1060).is_none(),
            "not again while silent"
        );
        let (s, a) = transition(s, false, 1300).unwrap();
        assert_eq!(s, Seen::Cycling);
        assert!(a.text.contains("300s"));
        assert!(transition(s, false, 1360).is_none());
    }
}
