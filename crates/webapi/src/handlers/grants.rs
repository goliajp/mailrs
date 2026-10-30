//! Two rules that keep an administrator from becoming more than they are.
//!
//! Only a super administrator acts on a super administrator's account: a
//! password reset, a recovery address, forwards or send-as on it would
//! hand its sign-in to whoever wrote them. And an administrator grants
//! only what they hold themselves, never `admin.*` or `internal.rpc`
//! unless they are super: a permission written into a group, or a group
//! someone is added to, is a grant.

use std::sync::Arc;

use axum::http::StatusCode;

use crate::WebState;

/// A permission only a super administrator may hand out.
pub fn reserved(permission: &str) -> bool {
    permission.starts_with("admin.") || permission == "internal.rpc" || permission == "*"
}

/// 403 unless `actor` may change `target`'s account.
pub async fn require_may_touch(
    state: &Arc<WebState>,
    actor: &str,
    target: &str,
) -> Result<(), StatusCode> {
    let target_super = match state.core.effective_permissions(target).await {
        Ok(p) => p.is_super,
        // not an account: nothing to protect, the handler answers for it
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => false,
        // unknown: treated as protected
        Err(_) => true,
    };
    if !target_super {
        return Ok(());
    }
    crate::handlers::kevy_util::require_super(state, actor).await
}

/// 403 unless `actor` may grant every one of `permissions`.
pub async fn require_may_grant(
    state: &Arc<WebState>,
    actor: &str,
    permissions: &[String],
) -> Result<(), StatusCode> {
    if permissions.is_empty() {
        return Ok(());
    }
    let mine = state
        .core
        .effective_permissions(actor)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if mine.is_super {
        return Ok(());
    }
    let allowed = permissions
        .iter()
        .all(|p| !reserved(p) && mine.permissions.iter().any(|m| m == p));
    match allowed {
        true => Ok(()),
        false => Err(StatusCode::FORBIDDEN),
    }
}

/// What a permission group grants its members.
pub fn group_permissions(id: i64) -> Result<Vec<String>, StatusCode> {
    let key = mailrs_core_sidestate::families::permission_groups::permissions_key(id);
    let raw = crate::handlers::kevy_util::with_kevy(move |c| {
        c.smembers(key.as_bytes()).map_err(std::io::Error::from)
    })?;
    Ok(raw
        .into_iter()
        .filter_map(|b| String::from_utf8(b).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::reserved;

    #[test]
    fn administrative_permissions_are_reserved() {
        for p in ["admin.accounts", "admin.cache", "internal.rpc", "*"] {
            assert!(reserved(p), "{p}");
        }
        for p in ["mail.read", "mail.send", "admin"] {
            assert!(!reserved(p), "{p}");
        }
    }
}
