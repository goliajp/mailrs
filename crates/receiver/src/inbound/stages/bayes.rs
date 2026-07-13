//! Bayesian spam-scoring stage (v2.8.1).
//!
//! Tokenizes the message via the `mailrs-bayes` stone, reads the
//! per-token spam/ham counts from the shared network kevy (trained by
//! fastcore's mark-junk / mark-not-junk hooks), and maps the resulting
//! spam probability onto `ctx.ai_score` — reusing the same score slot
//! `make_delivery_decision` already sums, so `decision.rs` needs no
//! change.
//!
//! Fail-open + cold-start safe: any kevy error, or an untrained corpus
//! (below `mailrs_bayes`'s cold-start gate), leaves `ai_score` at 0 —
//! zero effect on mail flow until the corpus has real training signal.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

use crate::kevy_net::KevyNetClient;

const K_SPAM: &[u8] = b"bayes:tokens:spam";
const K_HAM: &[u8] = b"bayes:tokens:ham";
const K_META: &[u8] = b"bayes:meta";

/// Bayesian classifier stage. Holds a network-kevy client to read the
/// token corpus. If no client is wired (kevy URL unset), the stage is
/// a no-op.
pub struct BayesStage {
    kevy: Option<Arc<KevyNetClient>>,
}

impl BayesStage {
    pub fn new(kevy: Option<Arc<KevyNetClient>>) -> Self {
        Self { kevy }
    }
}

#[async_trait]
impl Stage for BayesStage {
    fn name(&self) -> &str {
        "bayes"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let Some(kevy) = self.kevy.clone() else {
            return StageOutcome::Continue;
        };
        let tokens = mailrs_bayes::tokenize(&ctx.message);
        if tokens.is_empty() {
            return StageOutcome::Continue;
        }

        // Read corpus meta + per-token counts in one blocking round-trip.
        let tokens_moved = tokens.clone();
        let counts_result = tokio::task::spawn_blocking(move || {
            kevy.with_conn(|conn| fetch_counts(conn, &tokens_moved))
        })
        .await;

        let Ok(Ok((corpus, counts))) = counts_result else {
            return StageOutcome::Continue; // fail-open
        };

        let prob = mailrs_bayes::classify(&tokens, |t| counts.get(t).copied(), &corpus);
        if let Some(p) = prob {
            ctx.ai_score += map_score(p);
        }
        StageOutcome::Continue
    }
}

/// Map a spam probability to a score contribution. Conservative on the
/// high end (only a near-certain classification single-handedly clears
/// the default 5.0 threshold when combined with any minor rule hit),
/// and gives ham evidence a small negative offset to soften rule-engine
/// false positives.
fn map_score(p: f64) -> f64 {
    if p >= 0.95 {
        4.0
    } else if p >= 0.80 {
        2.5
    } else if p >= 0.60 {
        1.0
    } else if p <= 0.20 {
        -1.0
    } else {
        0.0
    }
}

type CountMap = std::collections::HashMap<String, mailrs_bayes::TokenCounts>;

/// HGET the meta corpus totals + HMGET (via pipelined HGET) the spam
/// and ham counts for every token. One connection, two token-count
/// batches.
fn fetch_counts(
    conn: &mut kevy_client::Connection,
    tokens: &[String],
) -> std::io::Result<(mailrs_bayes::Corpus, CountMap)> {
    let spam_msgs = hget_i64(conn, K_META, b"spam_msgs")?;
    let ham_msgs = hget_i64(conn, K_META, b"ham_msgs")?;
    let corpus = mailrs_bayes::Corpus {
        spam_msgs: spam_msgs.max(0) as u32,
        ham_msgs: ham_msgs.max(0) as u32,
    };

    let spam_counts = hmget_counts(conn, K_SPAM, tokens)?;
    let ham_counts = hmget_counts(conn, K_HAM, tokens)?;

    let mut map: CountMap = std::collections::HashMap::with_capacity(tokens.len());
    for (i, tok) in tokens.iter().enumerate() {
        let spam = spam_counts.get(i).copied().unwrap_or(0);
        let ham = ham_counts.get(i).copied().unwrap_or(0);
        if spam > 0 || ham > 0 {
            map.insert(
                tok.clone(),
                mailrs_bayes::TokenCounts {
                    spam: spam.max(0) as u32,
                    ham: ham.max(0) as u32,
                },
            );
        }
    }
    Ok((corpus, map))
}

fn hget_i64(conn: &mut kevy_client::Connection, key: &[u8], field: &[u8]) -> std::io::Result<i64> {
    Ok(conn
        .hget(key, field)?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

/// Pipelined per-token HGET against one corpus hash. Returns counts in
/// the same order as `tokens`.
fn hmget_counts(
    conn: &mut kevy_client::Connection,
    key: &[u8],
    tokens: &[String],
) -> std::io::Result<Vec<i64>> {
    let replies = conn.pipeline(|p| {
        for t in tokens {
            p.cmd(&[b"HGET", key, t.as_bytes()]);
        }
    })?;
    Ok(replies
        .into_iter()
        .map(|r| match r {
            kevy_client::Reply::Bulk(b) => std::str::from_utf8(&b)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            _ => 0,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_score_thresholds() {
        assert_eq!(map_score(0.99), 4.0);
        assert_eq!(map_score(0.95), 4.0);
        assert_eq!(map_score(0.85), 2.5);
        assert_eq!(map_score(0.65), 1.0);
        assert_eq!(map_score(0.50), 0.0);
        assert_eq!(map_score(0.10), -1.0);
    }

    #[test]
    fn no_client_is_noop() {
        // A stage with no kevy client must never touch ai_score.
        let stage = BayesStage::new(None);
        assert_eq!(stage.name(), "bayes");
    }
}
