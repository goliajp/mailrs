//! Replay the screening rules over stored mail, for the review that comes
//! before they act on anything.
//!
//! Read-only: nothing is moved, marked or written. Each conversation's
//! newest message is read back the way the receiver saw it — the
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

use super::fraud_rescan::reading::{header_value, newest_raw};
use super::prelude::*;

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
}

/// `POST /v1/admin/maintenance:screening-replay` — the body, when not
/// empty, is the Lua bundle under review; empty replays the built-in one.
pub(crate) async fn screening_replay_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<ReplayQuery>,
    body: String,
) -> axum::response::Response {
    // a Lua VM stays on the thread it was made on
    match tokio::task::spawn_blocking(move || replay(&state, &q, body)).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "screening replay panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn replay(state: &Arc<FastcoreState>, q: &ReplayQuery, body: String) -> axum::response::Response {
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
    let (mut clean, mut junk, mut held) = (Sample::default(), Sample::default(), Sample::default());
    let mut fraud = super::fraud_rescan::FraudJudge::new(state);
    let mut fraud_version = String::new();
    let (mut rows, mut errors) = (Vec::new(), Vec::new());
    let (mut walked, mut stopped_early, mut out_of_window) = (0u64, false, 0u64);
    for (i, (user, tid)) in targets.iter().enumerate() {
        if (i as u64) < q.skip {
            continue;
        }
        if walked >= q.limit {
            stopped_early = true;
            break;
        }
        walked += 1;
        if walked.is_multiple_of(PAUSE_EVERY) {
            std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
        }
        let Some((message_id, raw)) = newest_raw(state, user, tid) else {
            continue;
        };
        let arrived = mailrs_inbound::dates::received_epoch(&raw).unwrap_or(0);
        if now - arrived > q.days * DAY {
            out_of_window += 1;
            continue;
        }
        let row = state.mailbox.get_thread_for_user(user, tid).ok().flatten();
        let is_held = row.as_ref().is_some_and(|t| t.quarantined);
        let in_junk = row.as_ref().is_some_and(|t| {
            mailrs_mailbox_kevy::scope::bucket_of(&t.category)
                == mailrs_mailbox_kevy::scope::Bucket::Junk
        });
        let (name, sample) = match (is_held, in_junk) {
            (true, _) => ("held", &mut held),
            (false, true) => ("junk", &mut junk),
            (false, false) => ("clean", &mut clean),
        };
        sample.messages += 1;
        match fraud.judge(state, &message_id, &raw) {
            Ok(j) => {
                fraud_version = j.version;
                sample.fraud_hold += u64::from(j.holds);
                sample.fraud_junk += u64::from(j.junks);
                for f in j.findings.iter() {
                    *sample.fraud_by_rule.entry(f.rule.clone()).or_default() += 1;
                    let action = match f.holds {
                        true => "hold",
                        false => "score",
                    };
                    rows.push(Row {
                        message_id: message_id.clone(),
                        sample: name,
                        kind: "fraud",
                        rule: f.rule.clone(),
                        action: action.to_string(),
                        score: f.score,
                        detail: f.detail.clone(),
                        evidence: Vec::new(),
                    });
                }
            }
            Err(e) => errors.push(
                serde_json::json!({ "message_id": message_id, "error": format!("fraud: {e}") }),
            ),
        }
        let verdict = match judge(&mut rules, &set, &inspector, &own, &raw) {
            Ok(v) => v,
            Err(e) => {
                errors.push(serde_json::json!({ "message_id": message_id, "error": e }));
                continue;
            }
        };
        if verdict.is_empty() {
            continue;
        }
        sample.with_hits += 1;
        if verdict
            .iter()
            .any(|h| h.action == mailrs_screening_lua::Action::Junk)
        {
            sample.junk_action += 1;
        }
        for h in verdict {
            *sample.by_rule.entry(h.rule.clone()).or_default() += 1;
            rows.push(Row {
                message_id: message_id.clone(),
                sample: name,
                kind: "screening",
                rule: h.rule,
                action: format!("{:?}", h.action).to_ascii_lowercase(),
                score: h.score,
                detail: h.detail,
                evidence: h.evidence,
            });
        }
    }
    Json(serde_json::json!({
        "version": rules.version(),
        "fraud_version": fraud_version,
        "approved": mailrs_screening_lua::approved(rules.version()),
        "observations": set.observations(),
        "hashes": inspector.denied_hashes(),
        "days": q.days,
        "walked": walked,
        "out_of_window": out_of_window,
        "next_skip": q.skip + walked,
        "stopped_early": stopped_early,
        "clean": clean,
        "held": held,
        "junk": junk,
        "errors": errors,
        "hits": rows,
    }))
    .into_response()
}

/// The facts of one stored message, through the rules.
fn judge(
    rules: &mut mailrs_screening_lua::Rules,
    set: &mailrs_reputation::Set,
    inspector: &mailrs_attachment_scan::Inspector,
    own: &[String],
    raw: &[u8],
) -> Result<Vec<mailrs_screening_lua::Hit>, String> {
    let parsed = mailrs_mime::parse(raw);
    let (spf, _, dmarc) = mailrs_inbound::identity::auth_results_tokens(raw);
    let from = mailrs_inbound::identity::from_address(raw).to_ascii_lowercase();
    let header_domain = from
        .rsplit_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default();
    let envelope_domain = header_value(raw, b"return-path")
        .and_then(|v| {
            v.trim()
                .trim_matches(['<', '>'])
                .rsplit_once('@')
                .map(|(_, d)| d.to_ascii_lowercase())
        })
        .unwrap_or_default();
    let m = mailrs_reputation::Message {
        client_ip: mailrs_inbound::identity::first_received_peer_ip(raw),
        envelope_domain,
        header_domain: header_domain.clone(),
        link_hosts: mailrs_inbound::scan_input::link_hosts(&parsed),
        auth: mailrs_reputation::Auth {
            dmarc_fail: dmarc.eq_ignore_ascii_case("fail"),
            spf_fail: spf.eq_ignore_ascii_case("fail"),
        },
    };
    let reputation = set.facts(&m);
    let parts = mailrs_inbound::scan_input::attachments(&parsed);
    let atts: Vec<mailrs_attachment_scan::Attachment> = parts
        .iter()
        .map(|p| mailrs_attachment_scan::Attachment {
            name: &p.name,
            declared_type: &p.declared_type,
            bytes: p.bytes,
        })
        .collect();
    let parts = inspector.inspect(&atts);
    let ours = own.contains(&header_domain) && dmarc.eq_ignore_ascii_case("pass");
    let e = rules.evaluate(&mailrs_screening_lua::Input {
        outside: !ours,
        parts: &parts,
        reputation: &reputation,
    });
    match e.errors.is_empty() {
        true => Ok(e.hits),
        false => Err(e.errors.join("; ")),
    }
}
