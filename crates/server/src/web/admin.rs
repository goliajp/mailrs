use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{ApiResult, AuthUser, WebState};
use crate::message_util;

/// helper: check if user has a required permission, return error response if not
fn require_permission(
    permissions: &crate::permission::EffectivePermissions,
    perm: &str,
) -> Option<Json<ApiResult>> {
    if permissions.has(perm) {
        None
    } else {
        Some(Json(ApiResult {
            success: false,
            message: Some("insufficient permissions".into()),
        }))
    }
}

#[derive(Deserialize)]
pub(super) struct AddDomainRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub(super) struct AddAccountRequest {
    pub address: String,
    pub domain: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub recovery_email: String,
}

#[derive(Deserialize)]
pub(super) struct AddAliasRequest {
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
    #[serde(default = "default_alias_type")]
    pub alias_type: String,
}

fn default_alias_type() -> String {
    "alias".into()
}

#[derive(Serialize)]
pub(super) struct QuotaResponse {
    pub address: String,
    pub quota_bytes: i64,
}

#[derive(Deserialize)]
pub(super) struct SetQuotaRequest {
    pub quota_bytes: i64,
}

#[derive(Serialize)]
pub(super) struct SieveResponse {
    pub address: String,
    pub script: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct SetSieveRequest {
    pub script: String,
}

pub(super) async fn list_domains(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(Vec::<crate::domain_store::Domain>::new());
    };
    Json(ds.list_domains().await.unwrap_or_default())
}

pub(super) async fn add_domain(
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddDomainRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.domains") {
        return err;
    }
    if req.name.is_empty() || req.name.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid domain name length".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let now = chrono::Utc::now().timestamp();
    match ds.add_domain(&req.name, now).await {
        Ok(()) => {
            ds.log_audit(address, "domain_added", &req.name, "").await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn remove_domain(
    Path(name): Path<String>,
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.domains") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_domain(&name).await {
        Ok(true) => {
            ds.log_audit(address, "domain_removed", &name, "").await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("domain not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn check_domain_handler(
    Path(name): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref resolver) = state.resolver else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "DNS resolver not available"})),
        );
    };
    let report = crate::domain_check::check_domain(
        resolver,
        &name,
        state.dkim_selector.as_deref(),
        &state.hostname,
    )
    .await;
    match serde_json::to_value(report) {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to serialize domain check report: {e}")})),
        ),
    }
}

pub(super) async fn list_accounts(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(Vec::<crate::domain_store::Account>::new());
    }
    let Some(ref ds) = state.domain_store else {
        return Json(Vec::<crate::domain_store::Account>::new());
    };
    Json(ds.list_accounts().await.unwrap_or_default())
}

pub(super) async fn add_account(
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddAccountRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    if req.address.is_empty() || req.address.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid address length".into()),
        });
    }
    if req.domain.is_empty() || req.domain.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid domain length".into()),
        });
    }
    if req.display_name.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("display name too long".into()),
        });
    }
    if let Err(e) = crate::users::validate_email(&req.address) {
        return Json(ApiResult {
            success: false,
            message: Some(e.into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };

    // validate and hash password
    let password_hash = if req.password.is_empty() {
        String::new()
    } else {
        if let Err(e) = crate::users::validate_password(&req.password) {
            return Json(ApiResult {
                success: false,
                message: Some(e.into()),
            });
        }
        match crate::users::UserStore::hash_password(&req.password) {
            Ok(hash) => hash,
            Err(_) => return Json(ApiResult { success: false, message: Some("failed to hash password".into()) }),
        }
    };

    let now = chrono::Utc::now().timestamp();
    match ds
        .add_account(
            &req.address,
            &req.domain,
            &req.display_name,
            &password_hash,
            now,
        )
        .await
    {
        Ok(()) => {
            // update recovery_email if provided
            if !req.recovery_email.is_empty() {
                let _ = ds.update_recovery_email(&req.address, &req.recovery_email).await;
            }
            ds.log_audit(address, "account_created", &req.address, &format!("domain={}", req.domain)).await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn remove_account(
    Path(target_address): Path<String>,
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    // prevent admins from deleting their own account
    if target_address == *address {
        return Json(ApiResult {
            success: false,
            message: Some("cannot delete your own account".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_account(&target_address).await {
        Ok(true) => {
            ds.log_audit(address, "account_removed", &target_address, "").await;
            Json(ApiResult {
                success: true,
                message: None,
            })
        }
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("account not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn list_aliases(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    Json(serde_json::to_value(ds.list_aliases().await.unwrap_or_default()).unwrap_or_default())
}

pub(super) async fn add_alias(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddAliasRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.aliases") {
        return err;
    }
    if req.source_address.is_empty() || req.source_address.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid source address length".into()),
        });
    }
    if req.target_address.is_empty() || req.target_address.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid target address length".into()),
        });
    }
    if req.domain.is_empty() || req.domain.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid domain length".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let now = chrono::Utc::now().timestamp();
    match ds
        .add_alias(
            &req.source_address,
            &req.target_address,
            &req.domain,
            &req.alias_type,
            now,
        )
        .await
    {
        Ok(_) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn remove_alias(
    Path(id): Path<i64>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_alias(id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("alias not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn get_quota(
    Path(address): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "domain store not configured"})),
        )
            .into_response();
    };
    match ds.get_quota(&address).await {
        Ok(Some(quota_bytes)) => Json(QuotaResponse {
            address,
            quota_bytes,
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "account not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "operation failed"})),
            )
                .into_response()
        }
    }
}

pub(super) async fn set_quota(
    Path(address): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetQuotaRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.set_quota(&address, req.quota_bytes).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("account not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn get_sieve(
    Path(address): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "domain store not configured"})),
        )
            .into_response();
    };
    match ds.get_sieve_script(&address).await {
        Ok(script) => Json(SieveResponse { address, script }).into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "operation failed"})),
            )
                .into_response()
        }
    }
}

pub(super) async fn set_sieve(
    Path(address): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetSieveRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.sieve") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    if req.script.len() > super::MAX_SIEVE_SCRIPT_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("sieve script too large".into()),
        });
    }
    // validate sieve script before saving
    if let Err(e) = crate::sieve::compile_sieve(&req.script) {
        return Json(ApiResult {
            success: false,
            message: Some(format!("invalid sieve script: {e}")),
        });
    }
    let now = chrono::Utc::now().timestamp();
    match ds.set_sieve_script(&address, &req.script, now).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn delete_sieve(
    Path(address): Path<String>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.delete_sieve_script(&address).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("no sieve script found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

// ---------- MTA-STS ----------

pub(super) async fn mta_sts_policy(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref mode) = state.mta_sts_mode else {
        return (StatusCode::NOT_FOUND, "MTA-STS not configured".to_string());
    };

    let mx_lines: Vec<String> = state
        .mta_sts_mx
        .iter()
        .map(|mx| format!("mx: {mx}"))
        .collect();
    let body = format!(
        "version: STSv1\nmode: {mode}\n{}\nmax_age: {}\nid: {}",
        mx_lines.join("\n"),
        state.mta_sts_max_age,
        state.mta_sts_id
    );

    (StatusCode::OK, body)
}

// ---------- status + queue endpoints ----------

#[derive(serde::Serialize)]
pub(super) struct StatusResponse {
    pub uptime_secs: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub total_messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<QueueStatsResp>,
}

#[derive(serde::Serialize)]
pub(super) struct QueueStatsResp {
    pub pending: i64,
    pub inflight: i64,
    pub delivered: i64,
    pub failed: i64,
    pub bounced: i64,
}

#[derive(serde::Serialize)]
pub(super) struct QueueEntry {
    pub id: i64,
    pub sender: String,
    pub recipient: String,
    pub domain: String,
    pub status: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(serde::Serialize)]
pub(super) struct RetryResponse {
    pub success: bool,
    pub message: String,
}

pub(super) async fn prometheus_metrics(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    use std::fmt::Write;
    use std::sync::atomic::Ordering;

    let uptime = state.started_at.elapsed().as_secs();
    let total_connections = state.total_connections.load(Ordering::Relaxed);
    let active_connections = state.active_connections.load(Ordering::Relaxed);
    let total_messages = state.total_messages.load(Ordering::Relaxed);
    let active_sessions = state.sessions.len() as u64;
    let account_cache_size = state
        .domain_store
        .as_ref()
        .map(|ds| ds.cache_size())
        .unwrap_or(0) as u64;

    let (pg_up, valkey_up) = match &state.health {
        Some(h) => (h.pg_up(), h.valkey_up()),
        None => (false, false),
    };

    let (pending, delivered, failed, bounced) = if let Some(ref pool) = state.outbound_queue {
        match mailrs_outbound_queue::queue::queue_stats(pool).await {
            Ok(stats) => {
                let mut p = 0i64;
                let mut d = 0i64;
                let mut f = 0i64;
                let mut b = 0i64;
                for (status, count) in stats {
                    match status.as_str() {
                        "pending" | "inflight" => p += count,
                        "delivered" => d = count,
                        "failed" => f = count,
                        "bounced" => b = count,
                        _ => {}
                    }
                }
                (p, d, f, b)
            }
            Err(_) => (0, 0, 0, 0),
        }
    } else {
        (0, 0, 0, 0)
    };

    let mut body = String::with_capacity(1024);
    let _ = writeln!(body, "# HELP mailrs_uptime_seconds Time since server start");
    let _ = writeln!(body, "# TYPE mailrs_uptime_seconds gauge");
    let _ = writeln!(body, "mailrs_uptime_seconds {uptime}");
    let _ = writeln!(body, "# HELP mailrs_connections_total Total connections accepted");
    let _ = writeln!(body, "# TYPE mailrs_connections_total counter");
    let _ = writeln!(body, "mailrs_connections_total {total_connections}");
    let _ = writeln!(body, "# HELP mailrs_connections_active Currently open connections");
    let _ = writeln!(body, "# TYPE mailrs_connections_active gauge");
    let _ = writeln!(body, "mailrs_connections_active {active_connections}");
    let _ = writeln!(body, "# HELP mailrs_messages_total Total messages delivered locally");
    let _ = writeln!(body, "# TYPE mailrs_messages_total counter");
    let _ = writeln!(body, "mailrs_messages_total {total_messages}");
    let _ = writeln!(body, "# HELP mailrs_active_sessions Active web sessions");
    let _ = writeln!(body, "# TYPE mailrs_active_sessions gauge");
    let _ = writeln!(body, "mailrs_active_sessions {active_sessions}");
    let _ = writeln!(body, "# HELP mailrs_account_cache_size Domain store cache entries");
    let _ = writeln!(body, "# TYPE mailrs_account_cache_size gauge");
    let _ = writeln!(body, "mailrs_account_cache_size {account_cache_size}");
    let _ = writeln!(body, "# HELP mailrs_queue_pending Pending outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_pending gauge");
    let _ = writeln!(body, "mailrs_queue_pending {pending}");
    let _ = writeln!(body, "# HELP mailrs_queue_delivered Delivered outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_delivered gauge");
    let _ = writeln!(body, "mailrs_queue_delivered {delivered}");
    let _ = writeln!(body, "# HELP mailrs_queue_failed Failed outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_failed gauge");
    let _ = writeln!(body, "mailrs_queue_failed {failed}");
    let _ = writeln!(body, "# HELP mailrs_queue_bounced Bounced outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_bounced gauge");
    let _ = writeln!(body, "mailrs_queue_bounced {bounced}");
    let _ = writeln!(body, "# HELP mailrs_health_pg_up PostgreSQL availability");
    let _ = writeln!(body, "# TYPE mailrs_health_pg_up gauge");
    let _ = writeln!(body, "mailrs_health_pg_up {}", if pg_up { 1 } else { 0 });
    let _ = writeln!(body, "# HELP mailrs_health_valkey_up Valkey/Redis availability");
    let _ = writeln!(body, "# TYPE mailrs_health_valkey_up gauge");
    let _ = writeln!(body, "mailrs_health_valkey_up {}", if valkey_up { 1 } else { 0 });

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
}

pub(super) async fn get_status(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    use std::sync::atomic::Ordering;

    let queue = if let Some(ref pool) = state.outbound_queue {
        match mailrs_outbound_queue::queue::queue_stats(pool).await {
            Ok(stats) => {
                let mut qs = QueueStatsResp {
                    pending: 0,
                    inflight: 0,
                    delivered: 0,
                    failed: 0,
                    bounced: 0,
                };
                for (status, count) in stats {
                    match status.as_str() {
                        "pending" => qs.pending = count,
                        "inflight" => qs.inflight = count,
                        "delivered" => qs.delivered = count,
                        "failed" => qs.failed = count,
                        "bounced" => qs.bounced = count,
                        _ => {}
                    }
                }
                Some(qs)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    Json(StatusResponse {
        uptime_secs: state.started_at.elapsed().as_secs(),
        active_connections: state.active_connections.load(Ordering::Relaxed),
        total_connections: state.total_connections.load(Ordering::Relaxed),
        total_messages: state.total_messages.load(Ordering::Relaxed),
        queue,
    })
}

pub(super) async fn get_health(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let (status_label, level, pg, valkey, uptime) = match &state.health {
        Some(h) => (h.status_label(), h.level(), h.pg_up(), h.valkey_up(), h.uptime_secs()),
        None => ("unhealthy", 3, false, false, state.started_at.elapsed().as_secs()),
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": status_label,
            "level": level,
            "pg": pg,
            "valkey": valkey,
            "uptime_secs": uptime,
            "version": env!("CARGO_PKG_VERSION"),
            "active_sessions": state.sessions.len(),
            "account_cache_size": state.domain_store.as_ref().map(|ds| ds.cache_size()).unwrap_or(0),
            "total_connections": state.total_connections.load(std::sync::atomic::Ordering::Relaxed),
            "total_messages": state.total_messages.load(std::sync::atomic::Ordering::Relaxed),
        })),
    )
}

/// kubernetes-style readiness probe: returns 200 if PG is up, 503 otherwise
pub(super) async fn get_readiness(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let ready = state
        .health
        .as_ref()
        .map(|h| h.is_ready())
        .unwrap_or(false);
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(serde_json::json!({ "ready": ready })))
}

pub(super) async fn get_queue(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref pool) = state.outbound_queue else {
        return Json(Vec::<QueueEntry>::new());
    };

    let entries = match mailrs_outbound_queue::queue::list_recent(pool, 100).await {
        Ok(msgs) => msgs
            .into_iter()
            .map(|m| QueueEntry {
                id: m.id,
                sender: m.sender,
                recipient: m.recipient,
                domain: m.domain,
                status: m.status.as_str().to_string(),
                attempts: m.attempts,
                last_error: m.last_error,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect(),
        Err(_) => vec![],
    };

    Json(entries)
}

pub(super) async fn retry_queue_message(
    Path(id): Path<i64>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.outbound_queue else {
        return Json(RetryResponse {
            success: false,
            message: "outbound queue not configured".into(),
        });
    };

    let now = chrono::Utc::now().timestamp();
    match mailrs_outbound_queue::queue::retry_message(pool, id, now).await {
        Ok(true) => Json(RetryResponse {
            success: true,
            message: format!("message {id} queued for retry"),
        }),
        Ok(false) => Json(RetryResponse {
            success: false,
            message: format!("message {id} not found or not retryable"),
        }),
        Err(e) => Json(RetryResponse {
            success: false,
            message: format!("error: {e}"),
        }),
    }
}

// ---------- smtp config endpoint ----------

pub(super) async fn get_smtp_config(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    match &state.smtp_config {
        Some(cfg) => (StatusCode::OK, Json(serde_json::to_value(cfg).unwrap_or_default()))
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "smtp config not available"})),
        )
            .into_response(),
    }
}

// ---------- groups CRUD ----------

#[derive(Deserialize)]
pub(super) struct CreateGroupRequest {
    pub name: String,
    pub domain: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
pub(super) struct SetGroupPermissionsRequest {
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct AddMemberRequest {
    pub address: String,
}

#[derive(Deserialize)]
pub(super) struct SetOverridesRequest {
    pub overrides: Vec<OverrideEntry>,
}

#[derive(Deserialize)]
pub(super) struct OverrideEntry {
    pub permission: String,
    pub granted: bool,
}

pub(super) async fn list_groups(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let groups = ds.list_groups(None).await.unwrap_or_default();
    Json(serde_json::to_value(groups).unwrap_or_default())
}

pub(super) async fn create_group(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateGroupRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    if req.name.is_empty() || req.name.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid group name length".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds
        .add_group(&req.name, req.domain.as_deref(), &req.description)
        .await
    {
        Ok(id) => Json(ApiResult {
            success: true,
            message: Some(id.to_string()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn delete_group(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_group(id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("group not found or is builtin".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn get_group_permissions(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let perms = ds.get_group_permissions(id).await.unwrap_or_default();
    Json(serde_json::json!(perms))
}

pub(super) async fn set_group_permissions(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetGroupPermissionsRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    // validate permissions
    for perm in &req.permissions {
        if !crate::permission::ALL_PERMISSIONS.contains(&perm.as_str()) {
            return Json(ApiResult {
                success: false,
                message: Some(format!("unknown permission: {perm}")),
            });
        }
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.set_group_permissions(id, &req.permissions).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn list_group_members(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let members = ds.list_group_members(id).await.unwrap_or_default();
    Json(serde_json::json!(members))
}

pub(super) async fn add_group_member(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.add_account_to_group(&req.address, id).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn remove_group_member(
    Path((id, address)): Path<(i64, String)>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_account_from_group(&address, id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("membership not found".into()),
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn get_account_groups(
    Path(address): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let groups = ds.get_account_groups(&address).await.unwrap_or_default();
    Json(serde_json::to_value(groups).unwrap_or_default())
}

pub(super) async fn get_account_overrides(
    Path(address): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.groups") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let overrides = ds.get_account_overrides(&address).await.unwrap_or_default();
    let entries: Vec<serde_json::Value> = overrides
        .into_iter()
        .map(|(perm, granted)| serde_json::json!({"permission": perm, "granted": granted}))
        .collect();
    Json(serde_json::json!(entries))
}

pub(super) async fn set_account_overrides(
    Path(address): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetOverridesRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.groups") {
        return err;
    }
    // validate permissions
    for entry in &req.overrides {
        if !crate::permission::ALL_PERMISSIONS.contains(&entry.permission.as_str()) {
            return Json(ApiResult {
                success: false,
                message: Some(format!("unknown permission: {}", entry.permission)),
            });
        }
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let overrides: Vec<(String, bool)> = req
        .overrides
        .into_iter()
        .map(|e| (e.permission, e.granted))
        .collect();
    match ds.set_account_overrides(&address, &overrides).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult {
                success: false,
                message: Some("operation failed".into()),
        })
        },
    }
}

pub(super) async fn get_all_permissions(
    AuthUser { .. }: AuthUser,
) -> impl IntoResponse {
    Json(serde_json::json!(crate::permission::ALL_PERMISSIONS))
}

// ---------- email groups ----------

#[derive(Deserialize)]
pub(super) struct CreateEmailGroupRequest {
    pub address: String,
    pub domain: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub(super) async fn list_email_groups(
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let groups = ds.list_email_groups(None).await.unwrap_or_default();
    Json(serde_json::to_value(groups).unwrap_or_default())
}

pub(super) async fn create_email_group(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateEmailGroupRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    if req.address.is_empty() || req.address.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult { success: false, message: Some("invalid address".into()) });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.create_email_group(&req.address, &req.domain, &req.name, &req.description).await {
        Ok(id) => Json(ApiResult { success: true, message: Some(id.to_string()) }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult { success: false, message: Some("operation failed".into()) })
        },
    }
}

pub(super) async fn delete_email_group(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.remove_email_group(id).await {
        Ok(Some(_)) => Json(ApiResult { success: true, message: None }),
        Ok(None) => Json(ApiResult { success: false, message: Some("group not found".into()) }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult { success: false, message: Some("operation failed".into()) })
        },
    }
}

pub(super) async fn list_email_group_members(
    Path(id): Path<i64>,
    AuthUser { .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let members = ds.list_email_group_members(id).await.unwrap_or_default();
    Json(serde_json::json!(members))
}

pub(super) async fn add_email_group_member(
    Path(id): Path<i64>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    // prevent adding the group's own address as a member (would cause infinite delivery)
    if let Ok(groups) = ds.list_email_groups(None).await {
        if let Some(group) = groups.iter().find(|g| g.id == id) {
            if group.address == req.address {
                return Json(ApiResult { success: false, message: Some("cannot add group as member of itself".into()) });
            }
        }
    }
    match ds.add_email_group_member(id, &req.address).await {
        Ok(()) => Json(ApiResult { success: true, message: None }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult { success: false, message: Some("operation failed".into()) })
        },
    }
}

pub(super) async fn remove_email_group_member(
    Path((id, address)): Path<(i64, String)>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.remove_email_group_member(id, &address).await {
        Ok(true) => Json(ApiResult { success: true, message: None }),
        Ok(false) => Json(ApiResult { success: false, message: Some("member not found".into()) }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult { success: false, message: Some("operation failed".into()) })
        },
    }
}

// ---------- apps CRUD ----------

#[derive(Deserialize)]
pub(super) struct CreateAppRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// comma-separated scopes or array
    pub scopes: String,
}

pub(super) async fn list_apps(
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let apps = ds.list_apps(None).await.unwrap_or_default();
    Json(serde_json::to_value(apps).unwrap_or_default())
}

pub(super) async fn create_app(
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateAppRequest>,
) -> impl IntoResponse {
    if require_permission(permissions, "admin.accounts").is_some() {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "insufficient permissions"}))).into_response();
    }
    if req.name.is_empty() || req.name.len() > super::MAX_ADMIN_FIELD_LEN {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid app name"}))).into_response();
    }
    // validate scopes
    let scopes: Vec<&str> = req.scopes.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    for scope in &scopes {
        if !crate::permission::ALL_PERMISSIONS.contains(scope) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("unknown scope: {scope}")}))).into_response();
        }
    }

    let Some(ref ds) = state.domain_store else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "domain store not configured"}))).into_response();
    };

    let app_id = uuid::Uuid::new_v4().to_string();
    let scopes_str = scopes.join(",");

    match ds.create_app(&app_id, &req.name, &req.description, address, &scopes_str).await {
        Ok(id) => {
            ds.log_audit(address, "app_created", &req.name, &format!("app_id={app_id}")).await;
            // generate an initial API key for the app
            let Some(ref pool) = state.pg_pool else {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "db unavailable"}))).into_response();
            };

            let (full_key, prefix, key_hash) = crate::api_key_store::generate_api_key();
            match crate::api_key_store::insert_app_api_key(
                pool, &prefix, &key_hash, &full_key, address, &req.name, id, None,
            ).await {
                Ok(key_id) => {
                    (StatusCode::CREATED, Json(serde_json::json!({
                        "app_id": app_id,
                        "name": req.name,
                        "scopes": scopes_str,
                        "api_key": {
                            "id": key_id,
                            "key": full_key,
                            "prefix": prefix,
                        },
                    }))).into_response()
                }
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("app created but key generation failed: {e}")}))).into_response()
                }
            }
        }
        Err(e) => {
            {
                tracing::warn!(error = %e, "admin operation failed");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "operation failed"})))
            }.into_response()
        }
    }
}

pub(super) async fn get_app(
    Path(app_id): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "insufficient permissions"}))).into_response();
    }
    let Some(ref ds) = state.domain_store else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "domain store not configured"}))).into_response();
    };
    match ds.get_app(&app_id).await {
        Ok(Some(app)) => (StatusCode::OK, Json(serde_json::to_value(app).unwrap_or_default())).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "app not found"}))).into_response(),
        Err(e) => {
                tracing::warn!(error = %e, "admin operation failed");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "operation failed"})))
            }.into_response(),
    }
}

pub(super) async fn delete_app(
    Path(app_id): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.remove_app(&app_id).await {
        Ok(true) => Json(ApiResult { success: true, message: None }),
        Ok(false) => Json(ApiResult { success: false, message: Some("app not found".into()) }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult { success: false, message: Some("operation failed".into()) })
        },
    }
}

#[derive(Deserialize)]
pub(super) struct UpdateAppScopesRequest {
    pub scopes: String,
}

#[derive(Deserialize)]
pub(super) struct AuditLogQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: i64,
}

fn default_audit_limit() -> i64 {
    100
}

pub(super) async fn get_audit_log(
    Query(query): Query<AuditLogQuery>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.accounts") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let limit = query.limit.clamp(1, 1000);
    let entries = ds.list_audit_log(limit).await.unwrap_or_default();
    Json(serde_json::to_value(entries).unwrap_or_default())
}

pub(super) async fn update_app_scopes(
    Path(app_id): Path<String>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<UpdateAppScopesRequest>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.accounts") {
        return err;
    }
    // validate scopes
    let scopes: Vec<&str> = req.scopes.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    for scope in &scopes {
        if !crate::permission::ALL_PERMISSIONS.contains(scope) {
            return Json(ApiResult { success: false, message: Some(format!("unknown scope: {scope}")) });
        }
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.update_app_scopes(&app_id, &scopes.join(",")).await {
        Ok(true) => Json(ApiResult { success: true, message: None }),
        Ok(false) => Json(ApiResult { success: false, message: Some("app not found".into()) }),
        Err(e) => {
            tracing::warn!(error = %e, "admin operation failed");
            Json(ApiResult { success: false, message: Some("operation failed".into()) })
        },
    }
}

// ---------- mail audit ----------

/// validate that the auditor has admin.impersonate and the target user is in accessible domains
fn validate_audit_target(
    target_user: &str,
    permissions: &crate::permission::EffectivePermissions,
) -> Result<(), Json<ApiResult>> {
    if !permissions.has("admin.impersonate") {
        return Err(Json(ApiResult {
            success: false,
            message: Some("insufficient permissions".into()),
        }));
    }
    let domain = target_user
        .split_once('@')
        .map(|(_, d)| d)
        .unwrap_or("");
    if domain.is_empty() {
        return Err(Json(ApiResult {
            success: false,
            message: Some("invalid target user address".into()),
        }));
    }
    let accessible = permissions.accessible_domains();
    if !permissions.is_super() && !accessible.iter().any(|d| d == domain) {
        return Err(Json(ApiResult {
            success: false,
            message: Some("target user not in accessible domains".into()),
        }));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(super) struct AuditAccountsQuery {
    #[serde(default)]
    pub domain: Option<String>,
}

/// list accounts available for audit (filtered by accessible domains)
pub(super) async fn audit_list_accounts(
    Query(q): Query<AuditAccountsQuery>,
    AuthUser { ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !permissions.has("admin.impersonate") {
        return Json(serde_json::json!([]));
    }
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let accounts = ds.list_accounts().await.unwrap_or_default();
    let accessible = permissions.accessible_domains();
    let filtered: Vec<_> = accounts
        .into_iter()
        .filter(|a| {
            // domain scope check
            if !permissions.is_super() && !accessible.iter().any(|d| d == &a.domain) {
                return false;
            }
            // optional domain filter
            if let Some(ref d) = q.domain {
                return a.domain == *d;
            }
            true
        })
        .collect();
    Json(serde_json::to_value(filtered).unwrap_or_default())
}

#[derive(Deserialize)]
pub(super) struct AuditConversationsQuery {
    pub target_user: String,
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
}

/// list conversations for the target user (audit mode)
pub(super) async fn audit_list_conversations(
    Query(q): Query<AuditConversationsQuery>,
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Err(e) = validate_audit_target(&q.target_user, permissions) {
        return e.into_response();
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<super::conversations::ConversationResponse>::new()).into_response();
    };

    let limit = super::clamp_limit(q.limit);
    let convos = mb_store
        .list_conversations(
            &q.target_user,
            limit,
            q.before,
            q.category.as_deref(),
            None,
            false,
            q.folder.as_deref(),
        )
        .await
        .unwrap_or_default();

    // audit log
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(address, "audit.list_conversations", &q.target_user, "").await;
    }

    Json(super::conversations::convos_to_response(convos)).into_response()
}

/// get thread messages for the target user (audit mode)
pub(super) async fn audit_get_thread_messages(
    Path(thread_id): Path<String>,
    Query(q): Query<AuditTargetQuery>,
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Err(e) = validate_audit_target(&q.target_user, permissions) {
        return e.into_response();
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<super::conversations::ThreadMessageResponse>::new()).into_response();
    };

    if thread_id.len() > super::MAX_PATH_LEN {
        return Json(Vec::<super::conversations::ThreadMessageResponse>::new()).into_response();
    }

    let messages = mb_store
        .list_thread_messages(&q.target_user, &thread_id, None)
        .await
        .unwrap_or_default();

    let mut result = Vec::with_capacity(messages.len());
    for msg in &messages {
        let maildir_user = if msg.user_address.is_empty() {
            &q.target_user
        } else {
            &msg.user_address
        };
        let raw = message_util::read_message_raw(&state.maildir_root, maildir_user, &msg.maildir_id);
        let parsed = raw
            .as_deref()
            .map(message_util::parse_message)
            .unwrap_or_default();

        let (sender, subject) = if msg.sender.is_empty() || msg.subject.is_empty() {
            let raw_sender = raw
                .as_deref()
                .map(|d| message_util::extract_header_from_raw(d, "From"))
                .unwrap_or_default();
            let raw_subject = raw
                .as_deref()
                .map(|d| message_util::extract_header_from_raw(d, "Subject"))
                .unwrap_or_default();
            (
                if msg.sender.is_empty() {
                    message_util::decode_header(&raw_sender)
                } else {
                    message_util::decode_header(&msg.sender)
                },
                if msg.subject.is_empty() {
                    message_util::decode_header(&raw_subject)
                } else {
                    message_util::decode_header(&msg.subject)
                },
            )
        } else {
            (
                message_util::decode_header(&msg.sender),
                message_util::decode_header(&msg.subject),
            )
        };

        let ai = mb_store.get_email_analysis(msg.id).await.ok().flatten();
        let (
            category, risk_score, risk_reason, summary, people, dates, amounts,
            action_items, ai_analyzed, clean_text,
        ) = if let Some(ref a) = ai {
            let ct = if a.clean_text.is_empty() { None } else { Some(a.clean_text.clone()) };
            (
                a.category.clone(), a.risk_score as u8, a.risk_reason.clone(),
                a.summary.clone(), a.people.clone(), a.dates.clone(),
                a.amounts.clone(), a.action_items.clone(), true, ct,
            )
        } else {
            let (cat, score) =
                super::classify_email(&sender, &subject, parsed.0.as_deref(), parsed.1.as_deref());
            (
                cat, score, String::new(), String::new(),
                serde_json::json!([]), serde_json::json!([]),
                serde_json::json!([]), serde_json::json!([]),
                false, None,
            )
        };

        let structured_data = parsed.1.as_deref().and_then(|html| {
            let sd = crate::structured_data::extract_structured_data(html);
            if sd.is_empty() { None } else { Some(sd) }
        });

        result.push(super::conversations::ThreadMessageResponse {
            id: msg.id,
            uid: msg.uid,
            sender,
            recipients: msg.recipients.clone(),
            subject,
            flags: msg.flags,
            internal_date: msg.internal_date,
            message_id: msg.message_id.clone(),
            text_body: parsed.0,
            html_body: parsed.1,
            attachments: parsed.2,
            category,
            risk_score,
            risk_reason,
            summary,
            people,
            dates,
            amounts,
            action_items,
            ai_analyzed,
            clean_text,
            new_content: msg.new_content.clone(),
            importance_level: msg.importance_level.clone(),
            importance_score: msg.importance_score,
            is_bulk_sender: msg.is_bulk_sender,
            has_tracking_pixel: msg.has_tracking_pixel,
            requires_action: ai.as_ref().is_some_and(|a| a.requires_action),
            sender_intent: ai.as_ref().map_or_else(|| "inform".into(), |a| a.sender_intent.clone()),
            action_deadline: ai.as_ref().and_then(|a| a.action_deadline.clone()),
            structured_data,
        });
    }

    // audit log
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(address, "audit.read_thread", &q.target_user, &thread_id).await;
    }

    Json(result).into_response()
}

#[derive(Deserialize)]
pub(super) struct AuditTargetQuery {
    pub target_user: String,
}

/// get raw .eml for a target user's message (audit mode)
pub(super) async fn audit_get_raw_message(
    Path(uid): Path<u32>,
    Query(q): Query<AuditTargetQuery>,
    AuthUser { ref address, ref permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Err(e) = validate_audit_target(&q.target_user, permissions) {
        return e.into_response();
    }
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        })
        .into_response();
    };

    // find the message across the target user's mailboxes
    let mailboxes = mb_store
        .list_mailboxes(&q.target_user)
        .await
        .unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let maildir_user = &q.target_user;
            if let Some(data) = message_util::read_message_raw(
                &state.maildir_root,
                maildir_user,
                &msg.maildir_id,
            ) {
                // audit log
                if let Some(ref ds) = state.domain_store {
                    ds.log_audit(
                        address,
                        "audit.read_raw",
                        &q.target_user,
                        &uid.to_string(),
                    )
                    .await;
                }
                return (
                    StatusCode::OK,
                    [
                        (axum::http::header::CONTENT_TYPE, "message/rfc822"),
                        (
                            axum::http::header::CONTENT_DISPOSITION,
                            "attachment; filename=\"message.eml\"",
                        ),
                    ],
                    data,
                )
                    .into_response();
            }
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
    .into_response()
}
