//! Which way an alert goes.
//!
//! mailrs's own path first: the core queues the alert as mail to the
//! operator addresses and mailrs's sender delivers it (the operator's apps
//! hear of it as of any new mail). It counts as delivered only when the
//! sender's heartbeat lists the message's id; the core's 200 says queued,
//! nothing more. Without that marker within `MAILRS_OPS_OWN_PATH_FALLBACK_SECS`
//! (5 minutes by default) the message goes out again through the external
//! channels, when any are configured.
//!
//! An alert about the sending path itself (the sender, the core, the
//! receiver, kevy-server, the spool, the outbound queue) cannot trust that
//! path, so it goes straight to the external channels; only when there are
//! none is the own path tried anyway.

use super::{Channel, Dispatcher, Message, Sent, Which};
use crate::config::Config;

/// Said wherever the state is shown when no external channel is set.
pub const NO_EXTERNAL_WARNING: &str = "no external notification channel is configured: if mailrs itself is down, nobody is alerted, and a whole-host failure goes unnoticed";

/// Rules whose alert is about the own path itself.
const SENDING_PATH: &[&str] = &[
    "outbound_stuck",
    "spool_stalled",
    "core_store_damaged",
    crate::rules::KEVY_SERVER_UNAVAILABLE,
    "source_unreadable.fastcore",
    "source_unreadable.sender",
    "source_unreadable.receiver",
];

/// Processes the own path runs through, for the crash-loop rule.
const ON_THE_PATH: &[&str] = &["fastcore/", "sender/", "receiver/"];

/// Whether this alert may be sent by mailrs's own path.
pub fn via_own_path(n: &Sent) -> bool {
    let crash_on_path =
        n.rule == "crash_loop" && ON_THE_PATH.iter().any(|p| n.object.starts_with(p));
    !SENDING_PATH.contains(&n.rule) && !crash_on_path
}

/// The own path's channel, when it is turned on and the daemon has the
/// master key its credential is derived from.
pub(super) fn own_channel(cfg: &Config) -> Option<Channel> {
    if !cfg.own_path {
        return None;
    }
    let Some(master) = &cfg.master_key else {
        tracing::error!(
            "MAILRS_OPS_OWN_PATH is on but MAILRS_OPS_REPORT_MASTER_KEY is not set; own path off"
        );
        return None;
    };
    let key = mailrs_ops_report::key::derive(
        master.as_bytes(),
        mailrs_core_api::method::ops_alert::OPS_PROCESS,
    )?;
    Some(Channel::Own {
        url: format!(
            "{}{}",
            cfg.fastcore_url.trim_end_matches('/'),
            mailrs_core_api::method::ops_alert::PATH_OPS_ALERTS
        ),
        key,
    })
}

/// A message the own path took and the sender has not confirmed.
#[derive(Debug, Clone)]
pub(super) struct Pending {
    msg: Message,
    sent_at: i64,
}

impl Dispatcher {
    fn has(&self, own: bool) -> bool {
        self.slots
            .iter()
            .any(|s| s.disabled.is_none() && s.channel.is_own() == own)
    }

    /// Whether any external channel is enabled.
    pub fn has_external(&self) -> bool {
        self.has(false)
    }

    fn part(&mut self, of: &Message, notices: Vec<Sent>, now: i64) -> Message {
        self.message(of.kind.clone(), notices, now)
    }

    pub(super) async fn route(&mut self, msg: Message, now: i64) {
        if !self.has(true) {
            self.send_to(&msg, now, Which::External).await;
            return;
        }
        if msg.notices.is_empty() {
            self.own_first(msg, now).await;
            return;
        }
        let (own, outside): (Vec<Sent>, Vec<Sent>) =
            msg.notices.iter().cloned().partition(via_own_path);
        if !outside.is_empty() {
            let m = self.part(&msg, outside, now);
            if self.has_external() {
                self.send_to(&m, now, Which::External).await;
            } else {
                let why = "about the own path, and no external channel: own path tried";
                self.log(&m.id, "-", why.into(), &m.summary(), now);
                self.send_to(&m, now, Which::Own).await;
            }
        }
        if !own.is_empty() {
            let m = self.part(&msg, own, now);
            self.own_first(m, now).await;
        }
    }

    /// Own path; external at once if it does not take the message.
    async fn own_first(&mut self, m: Message, now: i64) {
        match self.send_to(&m, now, Which::Own).await {
            true => self.pending.push(Pending {
                msg: m,
                sent_at: now,
            }),
            false => {
                self.send_to(&m, now, Which::External).await;
            }
        }
    }

    /// The ids the sender's heartbeat says it delivered.
    pub fn mark_delivered(&mut self, ids: &[String], now: i64) {
        let (done, left): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| ids.contains(&p.msg.id));
        self.pending = left;
        for p in done {
            self.log(
                &p.msg.id,
                "own",
                "delivered (sender heartbeat)".into(),
                &p.msg.summary(),
                now,
            );
        }
    }

    /// Own-path messages still unconfirmed after the fallback window go out
    /// through the external channels.
    pub(super) async fn fall_back(&mut self, now: i64) {
        let (due, left): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| now - p.sent_at >= self.fallback_secs);
        self.pending = left;
        for p in due {
            let why = format!("no delivered marker within {}s", self.fallback_secs);
            match self.has_external() {
                true => {
                    self.log(
                        &p.msg.id,
                        "own",
                        format!("{why}: falling back"),
                        &p.msg.summary(),
                        now,
                    );
                    let m = self.message(p.msg.kind.clone(), p.msg.notices.clone(), now);
                    self.send_to(&m, now, Which::External).await;
                }
                false => self.log(
                    &p.msg.id,
                    "own",
                    format!("{why}; no external channel"),
                    &p.msg.summary(),
                    now,
                ),
            }
        }
    }

    /// Own-path messages awaiting the sender's marker.
    pub fn pending_ids(&self) -> Vec<String> {
        self.pending.iter().map(|p| p.msg.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests;
