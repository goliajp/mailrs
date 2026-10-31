//! The server's own observations behind the reputation judgement and the
//! attachment scan, recorded by a super administrator with a reason, and
//! the freshness of the sets built from them, for health.
//!
//! Observations are facts: added, never changed. A mistake is withdrawn by
//! recording a `retract` that names it.

use std::sync::{Arc, OnceLock};

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_sidestate::families::screening::{self, Live};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use crate::handlers::rules::refused;

static LIVE: OnceLock<Live> = OnceLock::new();
/// The receiver reports each minute; a report older than this means it
/// stopped.
const STALE_SECS: i64 = 180;
static RULES: std::sync::Mutex<Option<screening::RulesReport>> = std::sync::Mutex::new(None);

fn seq(id: &str) -> u64 {
    id.trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .unwrap_or(0)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Keep this process's view of the sets current, for health.
pub fn spawn_refresh() {
    let own = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    let _ = LIVE.set(Live::new(own));
    tokio::spawn(async move {
        loop {
            let read = with_kevy(|c| {
                LIVE.get().expect("set above").refresh(c, now());
                screening::rules_report(c)
            });
            match read {
                Ok(v) => *RULES.lock().expect("lock") = v,
                Err(status) => {
                    tracing::warn!(%status, "the screening rules report could not be read")
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
}

/// `reputation`, `attachment_scan` and `screening_rules`, for
/// `/api/health`. `screening_rules` is the receiver's report — the bundle
/// version, whether it is approved, its failure count — with `stale`
/// true when there is none or it is older than three minutes.
pub fn health() -> serde_json::Value {
    let mut h = LIVE.get().map_or(serde_json::Value::Null, Live::health);
    if let Some(obj) = h.as_object_mut() {
        let rules = RULES.lock().expect("lock").clone();
        let stale = rules
            .as_ref()
            .is_none_or(|r| now() - r.reported_at > STALE_SECS);
        let mut value = serde_json::json!(rules);
        match value.as_object_mut() {
            Some(o) => {
                o.insert("stale".into(), serde_json::json!(stale));
            }
            None => value = serde_json::json!({ "stale": true }),
        }
        obj.insert("screening_rules".into(), value);
    }
    h
}

#[derive(Debug, serde::Deserialize)]
pub struct ObservationRequest {
    pub source: mailrs_reputation::Source,
    pub kind: mailrs_reputation::Kind,
    pub value: String,
    pub stance: mailrs_reputation::Stance,
    #[serde(default)]
    pub retracts: Option<String>,
    pub reason: String,
}

/// GET /api/admin/screening/observations — every observation, newest first.
pub async fn list(
    State(_state): State<Arc<WebState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (mut obs, hashes) = with_kevy(|c| {
        Ok((
            screening::observations(c)?,
            screening::hash_observations(c)?,
        ))
    })?;
    // newest first; ids are assigned in order, so they break a tie
    obs.sort_by_key(|o| std::cmp::Reverse((o.at, seq(&o.id))));
    let mut hashes = hashes;
    hashes.sort_by_key(|o| std::cmp::Reverse((o.at, seq(&o.id))));
    Ok(Json(
        serde_json::json!({ "observations": obs, "hashes": hashes }),
    ))
}

/// POST /api/admin/screening/observations — record an incident, an allow
/// or a deny, or withdraw one. Only `incident` and `manual` are recorded
/// by hand; the other sources are written by the server itself.
pub async fn add(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<ObservationRequest>,
) -> Response {
    use mailrs_reputation::{Source, Stance};
    if !matches!(req.source, Source::Incident | Source::Manual) {
        return refused("source_not_by_hand", StatusCode::BAD_REQUEST);
    }
    if req.reason.trim().is_empty() {
        return refused("reason_required", StatusCode::BAD_REQUEST);
    }
    let retracting = req.stance == Stance::Retract;
    if retracting != req.retracts.is_some() || (!retracting && req.value.trim().is_empty()) {
        return refused("invalid_observation", StatusCode::BAD_REQUEST);
    }
    let o = mailrs_reputation::Observation {
        id: String::new(),
        at: now(),
        source: req.source,
        kind: req.kind,
        value: req.value.trim().to_ascii_lowercase(),
        stance: req.stance,
        retracts: req.retracts,
        reason: req.reason.trim().to_string(),
        actor: Some(actor.clone()),
    };
    let detail = format!("{:?} {:?} {:?} {}", o.source, o.stance, o.kind, o.value);
    match with_kevy(move |c| screening::add_observation(c, o)) {
        Ok(id) => {
            super::audit::record(&actor, "screening.observation", &id, &detail);
            (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response()
        }
        Err(status) => status.into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct HashRequest {
    pub sha256: String,
    pub stance: mailrs_attachment_scan::Stance,
    #[serde(default)]
    pub retracts: Option<String>,
    pub reason: String,
}

/// POST /api/admin/screening/hashes — a file known bad, a file known good
/// (the allowlist wins), or a withdrawal.
pub async fn add_hash(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<HashRequest>,
) -> Response {
    use mailrs_attachment_scan::Stance;
    let sha = req.sha256.trim().to_ascii_lowercase();
    let retracting = req.stance == Stance::Retract;
    let hex_ok = sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit());
    if req.reason.trim().is_empty()
        || retracting != req.retracts.is_some()
        || (!retracting && !hex_ok)
    {
        return refused("invalid_observation", StatusCode::BAD_REQUEST);
    }
    let o = mailrs_attachment_scan::HashObservation {
        id: String::new(),
        at: now(),
        sha256: sha,
        stance: req.stance,
        retracts: req.retracts,
        reason: req.reason.trim().to_string(),
        actor: Some(actor.clone()),
    };
    let detail = format!("{:?} {}", o.stance, o.sha256);
    match with_kevy(move |c| screening::add_hash_observation(c, o)) {
        Ok(id) => {
            super::audit::record(&actor, "screening.hash", &id, &detail);
            (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response()
        }
        Err(status) => status.into_response(),
    }
}
