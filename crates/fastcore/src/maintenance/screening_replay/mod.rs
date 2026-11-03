//! Replay the screening rules over stored mail, for the review that comes
//! before they act on anything.
//!
//! Read-only: nothing is moved, marked or written. With `scope=all` (the
//! default) every message in the window is replayed, attachments being
//! found as often in a conversation's middle as at its end; `scope=newest`
//! keeps the first round's reading, each conversation's newest message,
//! and its answer unchanged. A message is read back the way the receiver
//! saw it — the
//! connecting address from this deployment's own `Received:` line, SPF and
//! DMARC from its `Authentication-Results:` — and the facts go to the rule
//! bundle under review. Messages in Junk are the control sample; the rest
//! are the clean sample a false positive shows up in.
//!
//! The observations are today's, so a sender listed after a message
//! arrived counts against it: this overstates hits, the safe direction
//! for a false-positive count.
//!
//! The answer carries message ids, rules and their evidence (observation
//! ids, sources, ages, file names), never a body or attachment bytes.
//! Bounded like the other sweeps: `limit` conversations a call, a pause
//! every 25, `next_skip` in the answer.

use std::collections::BTreeMap;

mod coverage;
#[cfg(test)]
mod tests;
mod timing;

use super::fraud_rescan::reading::{newest_raw, raw_for_message};
use super::prelude::*;
use coverage::Coverage;
use mailrs_fraud_lua::stored::header_value;
use timing::ObservedAt;

const PAUSE_EVERY: u64 = 25;
const DAY: i64 = 86_400;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ReplayQuery {
    #[serde(default)]
    skip: u64,
    #[serde(default = "default_limit")]
    limit: u64,
    /// Only messages that arrived in the last `days` days.
    #[serde(default = "default_days")]
    days: i64,
    #[serde(default = "default_pause")]
    pause_ms: u64,
    /// `all` (default): every message in the window. `newest`: each
    /// conversation's newest message, the first round's reading.
    #[serde(default)]
    scope: Scope,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scope {
    #[default]
    All,
    Newest,
}

fn default_limit() -> u64 {
    2_000
}
fn default_days() -> i64 {
    90
}
fn default_pause() -> u64 {
    50
}

#[derive(Default, serde::Serialize)]
struct Sample {
    messages: u64,
    with_hits: u64,
    junk_action: u64,
    by_rule: BTreeMap<String, u64>,
    /// The fraud rules would hold it.
    fraud_hold: u64,
    /// The fraud rules would score it into Junk.
    fraud_junk: u64,
    fraud_by_rule: BTreeMap<String, u64>,
}

#[derive(serde::Serialize)]
struct Row {
    message_id: String,
    sample: &'static str,
    /// `screening` or `fraud`.
    kind: &'static str,
    rule: String,
    action: String,
    score: f64,
    detail: String,
    evidence: Vec<String>,
    /// `scope=all` only: every observation the hit cites was recorded
    /// after the message arrived.
    #[serde(skip_serializing_if = "Option::is_none")]
    after_the_fact: Option<bool>,
}

/// `POST /v1/admin/maintenance:screening-replay` — the body, when not
/// empty, is the Lua bundle under review; empty replays the built-in one.
pub(crate) async fn screening_replay_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<ReplayQuery>,
    body: String,
) -> axum::response::Response {
    // a Lua VM stays on the thread it was made on
    match super::hangup::run_blocking(move |h| replay(&state, &q, body, h)).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "screening replay panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn replay(
    state: &Arc<FastcoreState>,
    q: &ReplayQuery,
    body: String,
    hangup: &super::hangup::Hangup,
) -> axum::response::Response {
    let source = match body.trim().is_empty() {
        true => mailrs_screening_lua::DEFAULT_SOURCE.to_string(),
        false => body,
    };
    let mut rules = match mailrs_screening_lua::Rules::compile(&source) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };
    let now = chrono::Utc::now().timestamp();
    let Some(mut conn) = state.net_conn() else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no network kevy: observations cannot be read" })),
        )
            .into_response();
    };
    use mailrs_core_sidestate::families::screening;
    let (obs, hashes) = match (
        screening::observations(&mut conn),
        screening::hash_observations(&mut conn),
    ) {
        (Ok(o), Ok(h)) => (o, h),
        (Err(e), _) | (_, Err(e)) => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let own: Vec<String> = crate::spool_drain::relay::local_domains(state)
        .iter()
        .map(|d| d.to_ascii_lowercase())
        .collect();
    let set =
        mailrs_reputation::Set::build(&obs, &mailrs_reputation::NeverList::builtin(&own), now);
    let inspector = mailrs_attachment_scan::Inspector::build(&hashes, Default::default(), now);
    let targets = match super::fraud_rescan::snapshot(state) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "screening replay: snapshot failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let observed = ObservedAt::new(&obs, &hashes);
    let mut w = Walk {
        scope: q.scope,
        rules: &mut rules,
        set: &set,
        inspector: &inspector,
        own: &own,
        observed: &observed,
        fraud: super::fraud_rescan::FraudJudge::new(state),
        fraud_version: String::new(),
        samples: [Sample::default(), Sample::default(), Sample::default()],
        coverage: [Coverage::new(), Coverage::new(), Coverage::new()],
        rows: Vec::new(),
        errors: Vec::new(),
        out_of_window: 0,
    };
    let (mut walked, mut stopped_early) = (0u64, false);
    for (i, (user, tid)) in targets.iter().enumerate() {
        if (i as u64) < q.skip {
            continue;
        }
        if walked >= q.limit {
            stopped_early = true;
            break;
        }
        if hangup.hung_up() {
            tracing::warn!(walked, "screening replay: the caller hung up, stopping");
            stopped_early = true;
            break;
        }
        walked += 1;
        if walked.is_multiple_of(PAUSE_EVERY) {
            std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
        }
        let row = state.mailbox.get_thread_for_user(user, tid).ok().flatten();
        let is_held = row.as_ref().is_some_and(|t| t.quarantined);
        let in_junk = row.as_ref().is_some_and(|t| {
            mailrs_mailbox_kevy::scope::bucket_of(&t.category)
                == mailrs_mailbox_kevy::scope::Bucket::Junk
        });
        let which = match (is_held, in_junk) {
            (true, _) => 2,
            (false, true) => 1,
            (false, false) => 0,
        };
        for (message_id, raw) in messages(state, user, tid, q.scope) {
            let arrived = mailrs_inbound::dates::received_epoch(&raw).unwrap_or(0);
            if now - arrived > q.days * DAY {
                w.out_of_window += 1;
                continue;
            }
            w.message(state, which, user, &message_id, &raw, arrived);
        }
    }
    let [clean, junk, held] = w.samples;
    let mut out = serde_json::json!({
        "version": w.rules.version(),
        "fraud_version": w.fraud_version,
        "approved": mailrs_screening_lua::approved(w.rules.version()),
        "observations": set.observations(),
        "hashes": inspector.denied_hashes(),
        "days": q.days,
        "walked": walked,
        "out_of_window": w.out_of_window,
        "next_skip": q.skip + walked,
        "stopped_early": stopped_early,
        "clean": clean,
        "held": held,
        "junk": junk,
        "errors": w.errors,
        "hits": w.rows,
    });
    if q.scope == Scope::All {
        let [c, j, h] = w.coverage;
        out["scope"] = "all".into();
        out["coverage"] = serde_json::json!({ "clean": c, "junk": j, "held": h });
    }
    Json(out).into_response()
}

const SAMPLE_NAMES: [&str; 3] = ["clean", "junk", "held"];

/// The messages of `tid` the replay reads: every one this user holds, or
/// only the newest.
fn messages(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    scope: Scope,
) -> Vec<(String, Vec<u8>)> {
    match scope {
        Scope::Newest => newest_raw(state, user, tid).into_iter().collect(),
        Scope::All => state
            .mailbox
            .user_thread_message_ids(user, tid)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|mid| raw_for_message(state, user, &mid).map(|raw| (mid, raw)))
            .collect(),
    }
}

/// One replay's running totals.
struct Walk<'a> {
    scope: Scope,
    rules: &'a mut mailrs_screening_lua::Rules,
    set: &'a mailrs_reputation::Set,
    inspector: &'a mailrs_attachment_scan::Inspector,
    own: &'a [String],
    observed: &'a ObservedAt,
    fraud: super::fraud_rescan::FraudJudge,
    fraud_version: String,
    samples: [Sample; 3],
    coverage: [Coverage; 3],
    rows: Vec<Row>,
    errors: Vec<serde_json::Value>,
    out_of_window: u64,
}

impl Walk<'_> {
    /// Judge one message of sample `which` (clean, junk, held).
    fn message(
        &mut self,
        state: &Arc<FastcoreState>,
        which: usize,
        user: &str,
        message_id: &str,
        raw: &[u8],
        arrived: i64,
    ) {
        let name = SAMPLE_NAMES[which];
        let sample = &mut self.samples[which];
        sample.messages += 1;
        if self.scope == Scope::All {
            self.coverage[which].add(&coverage::seen(raw, self.inspector));
        }
        match self.fraud.judge(state, user, message_id, raw) {
            Ok(j) => {
                self.fraud_version = j.version;
                sample.fraud_hold += u64::from(j.holds);
                sample.fraud_junk += u64::from(j.junks);
                for f in j.findings.iter() {
                    *sample.fraud_by_rule.entry(f.rule.clone()).or_default() += 1;
                    let action = match f.holds {
                        true => "hold",
                        false => "score",
                    };
                    self.rows.push(Row {
                        message_id: message_id.to_string(),
                        sample: name,
                        kind: "fraud",
                        rule: f.rule.clone(),
                        action: action.to_string(),
                        score: f.score,
                        detail: f.detail.clone(),
                        evidence: Vec::new(),
                        after_the_fact: None,
                    });
                }
            }
            Err(e) => self.errors.push(
                serde_json::json!({ "message_id": message_id, "error": format!("fraud: {e}") }),
            ),
        }
        let verdict = match judge(self.rules, self.set, self.inspector, self.own, raw) {
            Ok(v) => v,
            Err(e) => {
                self.errors
                    .push(serde_json::json!({ "message_id": message_id, "error": e }));
                return;
            }
        };
        let (mut in_time, mut junk_in_time) = (false, false);
        for h in verdict {
            let late = match self.scope {
                Scope::All => Some(self.observed.after_the_fact(&h.evidence, arrived)),
                Scope::Newest => None,
            };
            if late == Some(true) {
                let c = &mut self.coverage[which];
                c.after_the_fact += 1;
                *c.after_the_fact_by_rule.entry(h.rule.clone()).or_default() += 1;
            } else {
                in_time = true;
                junk_in_time |= h.action == mailrs_screening_lua::Action::Junk;
                *sample.by_rule.entry(h.rule.clone()).or_default() += 1;
            }
            self.rows.push(Row {
                message_id: message_id.to_string(),
                sample: name,
                kind: "screening",
                rule: h.rule,
                action: format!("{:?}", h.action).to_ascii_lowercase(),
                score: h.score,
                detail: h.detail,
                evidence: h.evidence,
                after_the_fact: late,
            });
        }
        sample.with_hits += u64::from(in_time);
        sample.junk_action += u64::from(junk_in_time);
    }
}

/// The facts of one stored message, through the rules.
fn judge(
    rules: &mut mailrs_screening_lua::Rules,
    set: &mailrs_reputation::Set,
    inspector: &mailrs_attachment_scan::Inspector,
    own: &[String],
    raw: &[u8],
) -> Result<Vec<mailrs_screening_lua::Hit>, String> {
    let envelope_domain = header_value(raw, b"return-path")
        .and_then(|v| {
            v.trim()
                .trim_matches(['<', '>'])
                .rsplit_once('@')
                .map(|(_, d)| d.to_ascii_lowercase())
        })
        .unwrap_or_default();
    let e = mailrs_screening_lua::stored::with_input(
        set,
        inspector,
        own,
        raw,
        &envelope_domain,
        |input| rules.evaluate(input),
    );
    match e.errors.is_empty() {
        true => Ok(e.hits),
        false => Err(e.errors.join("; ")),
    }
}
