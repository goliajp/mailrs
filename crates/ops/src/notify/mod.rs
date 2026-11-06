//! From notices to messages: silences, maintenance, grouping, the hourly
//! budget per channel, and the record of every send.

mod channels;
mod held;
mod render;
mod route;

use std::sync::Arc;

pub use channels::{Channel, mail_bytes, relay_refusal};
pub use route::{ACCEPTED_250, NO_EXTERNAL_WARNING, via_own_path};

use crate::config::Config;
use crate::engine::Notice;
use crate::readings::ChannelView;
use crate::rules::Text;
use crate::store::{NotificationRow, Silence, Store};

/// A notice as it goes out.
#[derive(Debug, Clone)]
pub struct Sent {
    pub rule: &'static str,
    pub kind: &'static str,
    pub severity: &'static str,
    pub object_kind: &'static str,
    pub object: String,
    pub value: String,
    pub threshold: String,
    pub since: i64,
    pub text: &'static Text,
    pub under: Option<&'static str>,
}

impl From<&Notice> for Sent {
    fn from(n: &Notice) -> Self {
        Sent {
            rule: n.rule.name,
            kind: n.kind.as_str(),
            severity: n.severity.as_str(),
            object_kind: n.rule.object_kind,
            object: n.object.clone(),
            value: n.value.clone(),
            threshold: n.threshold.clone(),
            since: n.since,
            text: n.rule.text,
            under: n.under,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    Alerts,
    Digest { skipped: usize },
    Test,
    MaintenanceEnded { held: usize },
}

impl MessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageKind::Alerts => "alerts",
            MessageKind::Digest { .. } => "digest",
            MessageKind::Test => "test",
            MessageKind::MaintenanceEnded { .. } => "maintenance_ended",
        }
    }

    /// Tests and digests do not use up a channel's hourly budget.
    fn counted(&self) -> bool {
        matches!(
            self,
            MessageKind::Alerts | MessageKind::MaintenanceEnded { .. }
        )
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub at: i64,
    pub host: String,
    pub kind: MessageKind,
    pub notices: Vec<Sent>,
    pub link: Option<String>,
}

struct Slot {
    channel: Channel,
    /// Why the channel is not used, decided at start.
    disabled: Option<String>,
    failures: u32,
    hour: i64,
    sent: u32,
    skipped: usize,
}

pub struct Dispatcher {
    slots: Vec<Slot>,
    http: reqwest::Client,
    outbox: Vec<Sent>,
    outbox_since: Option<i64>,
    store: Arc<Store>,
    group_secs: i64,
    max_per_hour: u32,
    minimal: bool,
    link: Option<String>,
    host: String,
    seq: u64,
    /// Sent by the own path and not yet confirmed by the sender.
    pending: Vec<route::Pending>,
    fallback_secs: i64,
}

/// Which slots a message goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Own,
    External,
}

impl Dispatcher {
    pub fn new(cfg: &Config, store: Arc<Store>) -> Self {
        let mut channels = Vec::new();
        if let Some(own) = route::own_channel(cfg) {
            channels.push(own);
        }
        if let Some(url) = &cfg.webhook_url {
            channels.push(Channel::Webhook {
                url: url.clone(),
                secret: cfg.webhook_secret.clone(),
            });
        }
        if let Some(url) = &cfg.push_url {
            channels.push(Channel::Push {
                url: url.clone(),
                token: cfg.push_token.clone(),
            });
        }
        if let Some(relay) = &cfg.relay {
            channels.push(Channel::Relay(relay.clone()));
        }
        let slots = channels
            .into_iter()
            .map(|channel| {
                let disabled = match &channel {
                    Channel::Relay(r) => relay_refusal(r, &cfg.local_domains),
                    _ => None,
                };
                if let Some(why) = &disabled {
                    tracing::error!(channel = channel.name(), %why, "notification channel not enabled");
                }
                Slot {
                    channel,
                    disabled,
                    failures: 0,
                    hour: 0,
                    sent: 0,
                    skipped: 0,
                }
            })
            .collect();
        Dispatcher {
            slots,
            http: reqwest::Client::new(),
            outbox: Vec::new(),
            outbox_since: None,
            store,
            group_secs: cfg.group_secs,
            max_per_hour: cfg.max_per_hour,
            minimal: cfg.push_minimal,
            link: cfg.admin_url.clone(),
            host: cfg.hostname.clone(),
            seq: 0,
            pending: Vec::new(),
            fallback_secs: cfg.own_path_fallback_secs,
        }
    }

    fn log(&self, group: &str, channel: &str, result: String, summary: &str, now: i64) {
        let row = NotificationRow {
            group_id: group.to_string(),
            channel: channel.to_string(),
            attempt: 1,
            result,
            at: now,
            summary: summary.to_string(),
        };
        if let Err(e) = self.store.record_notification(&row) {
            tracing::error!(error = %e, "notification not logged");
        }
    }

    /// Take this cycle's notices: file, silence or hold each, queue the rest.
    pub fn accept(
        &mut self,
        notices: &[Notice],
        silences: &[Silence],
        maintenance: bool,
        now: i64,
    ) {
        for n in notices {
            let sent = Sent::from(n);
            let line = format!("{} {} {}", sent.kind, sent.rule, sent.object);
            let silenced = silences.iter().find(|s| {
                s.rule == sent.rule && s.object.as_ref().is_none_or(|o| *o == sent.object)
            });
            match (n.under, silenced, maintenance && !n.rule.during_maintenance) {
                (Some(parent), _, _) => {
                    self.log("-", "-", format!("filed under {parent}"), &line, now)
                }
                (None, Some(s), _) => {
                    self.log("-", "-", format!("silenced by {}", s.id), &line, now)
                }
                (None, None, true) => {
                    self.log("-", "-", "held for maintenance".into(), &line, now);
                    if let Err(e) = self.store.record_held(&sent.held_row(now)) {
                        tracing::error!(error = %e, "held notice not recorded");
                    }
                }
                (None, None, false) => {
                    self.outbox_since.get_or_insert(now);
                    self.outbox.push(sent);
                }
            }
        }
    }

    fn message(&mut self, kind: MessageKind, notices: Vec<Sent>, now: i64) -> Message {
        self.seq += 1;
        Message {
            id: format!("{now}-{}", self.seq),
            at: now,
            host: self.host.clone(),
            kind,
            notices,
            link: self.link.clone(),
        }
    }

    /// Send the queued group once its window has passed, and each
    /// channel's digest once the hour that ran out of budget is over.
    pub async fn tick(&mut self, now: i64) {
        self.fall_back(now).await;
        let hour = now / 3600;
        for i in 0..self.slots.len() {
            let (old, skipped) = (self.slots[i].hour, self.slots[i].skipped);
            if old != hour && skipped > 0 {
                let digest = self.message(MessageKind::Digest { skipped }, Vec::new(), now);
                self.send_one(i, &digest, now).await;
            }
            if old != hour {
                let s = &mut self.slots[i];
                (s.hour, s.sent, s.skipped) = (hour, 0, 0);
            }
        }
        let due = self
            .outbox_since
            .is_some_and(|t| now - t >= self.group_secs);
        if due {
            let notices = std::mem::take(&mut self.outbox);
            self.outbox_since = None;
            let msg = self.message(MessageKind::Alerts, notices, now);
            self.route(msg, now).await;
        }
    }

    /// What the channels are configured to reach, as a hash: a test goes out
    /// whenever it changes. `None` with no channel at all.
    pub fn config_fingerprint(&self) -> Option<String> {
        use sha2::Digest;
        if self.slots.is_empty() {
            return None;
        }
        let described: Vec<String> = self
            .slots
            .iter()
            .map(|s| match &s.channel {
                Channel::Webhook { url, .. } => format!("webhook {url}"),
                Channel::Push { url, .. } => format!("push {url}"),
                Channel::Relay(r) => format!("relay {}:{} {}", r.host, r.port, r.to.join(",")),
                Channel::Own { url, .. } => format!("own {url}"),
            })
            .collect();
        Some(hex::encode(sha2::Sha256::digest(
            described.join("\n").as_bytes(),
        )))
    }

    /// A test message to every enabled channel; `(channel, result)`.
    pub async fn test(&mut self, now: i64) -> Vec<(String, String)> {
        let msg = self.message(MessageKind::Test, Vec::new(), now);
        let mut out = Vec::new();
        for i in 0..self.slots.len() {
            let result = match &self.slots[i].disabled {
                Some(why) => format!("disabled: {why}"),
                None => self.send_one(i, &msg, now).await,
            };
            out.push((self.slots[i].channel.name().to_string(), result));
        }
        out
    }

    /// Send `msg` to the slots `which` picks; whether the own path took it.
    async fn send_to(&mut self, msg: &Message, now: i64, which: Which) -> bool {
        let mut own_took = false;
        for i in 0..self.slots.len() {
            let own = self.slots[i].channel.is_own();
            let picked = match which {
                Which::Own => own,
                Which::External => !own,
            };
            if !picked {
                continue;
            }
            let name = self.slots[i].channel.name();
            // a channel's own failure goes out through the others
            let about_itself = msg
                .notices
                .iter()
                .any(|n| n.rule == crate::rules::CHANNEL_FAILING && n.object == name);
            if self.slots[i].disabled.is_some() || about_itself {
                continue;
            }
            if msg.kind.counted() && self.slots[i].sent >= self.max_per_hour {
                self.slots[i].skipped += msg.notices.len().max(1);
                self.log(&msg.id, name, "over budget".into(), &msg.summary(), now);
                continue;
            }
            let sent = self.send_one(i, msg, now).await == "sent";
            own_took |= own && sent;
        }
        own_took
    }

    async fn send_one(&mut self, i: usize, msg: &Message, now: i64) -> String {
        let result = self.slots[i]
            .channel
            .send(msg, &self.http, self.minimal)
            .await;
        let slot = &mut self.slots[i];
        let logged = match result {
            Ok(()) => {
                slot.failures = 0;
                if msg.kind.counted() {
                    slot.sent += 1;
                }
                "sent".to_string()
            }
            Err(e) => {
                slot.failures += 1;
                tracing::warn!(channel = slot.channel.name(), error = %e, "notification not sent");
                format!("failed: {e}")
            }
        };
        let name = slot.channel.name();
        self.log(&msg.id, name, logged.clone(), &msg.summary(), now);
        logged
    }

    pub fn channel_views(&self) -> Vec<ChannelView> {
        self.slots
            .iter()
            .filter(|s| s.disabled.is_none())
            .map(|s| ChannelView {
                name: s.channel.name().to_string(),
                consecutive_failures: s.failures,
            })
            .collect()
    }

    pub fn status(&self) -> serde_json::Value {
        let channels: Vec<_> = self
            .slots
            .iter()
            .map(|s| {
                serde_json::json!({
                    "channel": s.channel.name(),
                    "enabled": s.disabled.is_none(),
                    "disabled_because": s.disabled,
                    "consecutive_failures": s.failures,
                    "sent_this_hour": s.sent,
                    "not_sent_this_hour": s.skipped,
                })
            })
            .collect();
        serde_json::json!({
            "channels": channels,
            "queued": self.outbox.len(),
            "own_path_awaiting_acceptance_250": self.pending_ids(),
            "push_minimal": self.minimal,
        })
    }
}

#[cfg(test)]
mod tests;
