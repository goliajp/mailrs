//! The SMTP live monitor's wire contract: two pub/sub channel names and
//! the counter frame.
//!
//! It lives here for the reason `sieve_key` does — the writer is
//! `mailrs-receiver` and the reader is `mailrs-webapi`, they share no
//! other crate, and a channel name spelled twice is a channel name that
//! will eventually be spelled two ways. That failure is silent on both
//! sides: the publisher publishes to nobody and the subscriber waits on
//! a channel nothing writes, which is indistinguishable from an idle
//! mail server.
//!
//! Deliberately **pub/sub, not the change feed**. A protocol trace is
//! diagnostic and only wanted while somebody has the monitor open; at
//! roughly a hundred connections an hour times a dozen command lines
//! each, putting it in the durable feed would write tens of thousands of
//! AOF entries a day that no reader ever asks for.

use serde::{Deserialize, Serialize};

/// One SMTP session's protocol trace: `ConnectionOpened` through
/// `ConnectionClosed`, serialised as `SmtpEvent` inside the receiver's
/// notify envelope.
pub const TRACE_CHANNEL: &[u8] = b"trace:smtp";

/// The receiver's four counters. A separate channel so a reader tells a
/// stats frame from a trace frame by the channel it arrived on, rather
/// than by a discriminator inside the payload.
pub const STATS_CHANNEL: &[u8] = b"trace:smtp-stats";

/// What the monitor's four cards render.
///
/// Absent is a real state and means *no receiver has published yet* — the
/// reader shows a dash for it, never a zero. `0` and "unknown" are
/// different answers, and only one of them is true of a mail server that
/// has just started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverStats {
    pub active_connections: u64,
    pub total_connections: u64,
    pub total_messages: u64,
    pub uptime_secs: u64,
    /// RCPTs refused since start because the sender guessed too many
    /// unknown local addresses. `None` from a receiver older than the
    /// field: unknown, not zero.
    #[serde(default)]
    pub probe_guard_tripped: Option<u64>,
    /// Submissions from a masked address the submission log could not
    /// record since start; `None` as above.
    #[serde(default)]
    pub mask_submit_log_failed: Option<u64>,
    /// The TLSA records port 25 ought to have and what DNS last said;
    /// `None` when the receiver serves no certificate or predates the field.
    #[serde(default)]
    pub dane: Option<DaneStatus>,
    /// The receiver's parse workers; `None` from a receiver that predates
    /// them.
    #[serde(default)]
    pub parsing: Option<mailrs_core_api::types::ParsingHealthWire>,
    /// Unix seconds the receiver built this frame; `None` from a receiver
    /// that predates it. With the time the reader got it, a frame that is
    /// only the last one before the receiver stopped can be told apart.
    #[serde(default)]
    pub sent_at: Option<i64>,
}

/// The receiver's view of DANE for its own hostname.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaneStatus {
    /// `_25._tcp.<hostname>`
    pub name: String,
    /// `3 1 1 <hex>` for the certificate being served; `None` when the
    /// file could not be read or parsed
    pub current: Option<String>,
    /// The same for the key that is to replace it, when one is configured.
    pub next: Option<String>,
    /// Unix seconds of the last DNS check; `None` before the first.
    pub checked_at: Option<u64>,
    pub sources: Vec<DaneSource>,
    /// Certificate changes since start whose new certificate matched none
    /// of the records then published.
    pub unmatched_swaps: u64,
}

/// One server's answer for the TLSA name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaneSource {
    pub server: String,
    /// Whether the resolver validated the answer (the AD bit); `None` for
    /// an authoritative server, which does not validate.
    pub authenticated: Option<bool>,
    /// `usage selector matching hex`, lowercase, sorted.
    pub records: Vec<String>,
    pub error: Option<String>,
}

impl DaneStatus {
    /// The records counted as published: what every validating resolver
    /// returned, and only when each of them answered with AD set. A record
    /// one resolver has and another lacks is not published yet.
    pub fn published(&self) -> Vec<String> {
        let mut validating = self.sources.iter().filter(|s| s.authenticated.is_some());
        let Some(first) = validating.next() else {
            return Vec::new();
        };
        let all_validated = self
            .sources
            .iter()
            .filter(|s| s.authenticated.is_some())
            .all(|s| s.error.is_none() && s.authenticated == Some(true));
        if !all_validated {
            return Vec::new();
        }
        first
            .records
            .iter()
            .filter(|r| validating.clone().all(|s| s.records.contains(r)))
            .cloned()
            .collect()
    }

    pub fn matches(&self, record: Option<&String>) -> bool {
        record.is_some_and(|r| self.published().contains(r))
    }

    /// Whether every server that answered returned the same records.
    pub fn sources_agree(&self) -> bool {
        let mut answered = self.sources.iter().filter(|s| s.error.is_none());
        match answered.next() {
            Some(first) => answered.all(|s| s.records == first.records),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round trip the two processes actually perform. If a field is
    /// renamed on one side this is what fails, rather than the monitor
    /// quietly showing dashes.
    #[test]
    fn stats_round_trip_over_the_wire() {
        let sent = ReceiverStats {
            active_connections: 2,
            total_connections: 483,
            total_messages: 17,
            uptime_secs: 18_346,
            probe_guard_tripped: Some(3),
            mask_submit_log_failed: Some(1),
            dane: Some(DaneStatus {
                name: "_25._tcp.mx.example".into(),
                current: Some("3 1 1 ab".into()),
                next: None,
                checked_at: Some(1),
                sources: vec![],
                unmatched_swaps: 0,
            }),
            parsing: Some(mailrs_core_api::types::ParsingHealthWire {
                worker: true,
                failures_24h: 1,
                last_failure_at: Some(1_700_000_000),
                worker_restarts: 1,
                failures: vec![mailrs_core_api::types::ParserFailureWire {
                    parser: "html2text".into(),
                    kind: "panic".into(),
                    count: 1,
                }],
                paused_since: None,
            }),
            sent_at: Some(1_700_000_000),
        };
        let bytes = serde_json::to_vec(&sent).unwrap();
        let got: ReceiverStats = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(sent, got);
    }

    /// The channel names are the contract; pin them so a rename has to be
    /// deliberate on both sides at once.
    #[test]
    fn the_channel_names_are_pinned() {
        assert_eq!(TRACE_CHANNEL, b"trace:smtp");
        assert_eq!(STATS_CHANNEL, b"trace:smtp-stats");
        assert_ne!(TRACE_CHANNEL, STATS_CHANNEL);
    }

    fn source(server: &str, ad: Option<bool>, records: &[&str]) -> DaneSource {
        DaneSource {
            server: server.into(),
            authenticated: ad,
            records: records.iter().map(|r| r.to_string()).collect(),
            error: None,
        }
    }

    fn status(sources: Vec<DaneSource>) -> DaneStatus {
        DaneStatus {
            name: "_25._tcp.mx.example".into(),
            current: Some("3 1 1 aa".into()),
            next: Some("3 1 1 bb".into()),
            checked_at: Some(1),
            sources,
            unmatched_swaps: 0,
        }
    }

    #[test]
    fn a_record_is_published_only_once_every_validating_resolver_returns_it_with_ad() {
        let both = status(vec![
            source("1.1.1.1", Some(true), &["3 1 1 aa", "3 1 1 bb"]),
            source("8.8.8.8", Some(true), &["3 1 1 aa"]),
            source("ns", None, &["3 1 1 aa", "3 1 1 bb"]),
        ]);
        assert_eq!(both.published(), vec!["3 1 1 aa".to_string()]);
        assert!(both.matches(both.current.as_ref()));
        assert!(
            !both.matches(both.next.as_ref()),
            "one resolver still lacks it"
        );
        assert!(!both.sources_agree());

        let unsigned = status(vec![
            source("1.1.1.1", Some(false), &["3 1 1 aa"]),
            source("8.8.8.8", Some(true), &["3 1 1 aa"]),
        ]);
        assert!(
            unsigned.published().is_empty(),
            "no AD on one side is not published"
        );

        let mut failed = status(vec![
            source("1.1.1.1", Some(true), &["3 1 1 aa"]),
            source("8.8.8.8", Some(true), &["3 1 1 aa"]),
        ]);
        failed.sources[1].error = Some("timed out".into());
        assert!(
            failed.published().is_empty(),
            "an unanswered resolver is not agreement"
        );
    }

    #[test]
    fn nothing_published_matches_nothing() {
        let none = status(vec![
            source("1.1.1.1", Some(true), &[]),
            source("8.8.8.8", Some(true), &[]),
            source("ns", None, &[]),
        ]);
        assert!(none.published().is_empty());
        assert!(!none.matches(none.current.as_ref()));
        assert!(none.sources_agree());
        assert!(!status(vec![]).matches(Some(&"3 1 1 aa".to_string())));
    }
}
