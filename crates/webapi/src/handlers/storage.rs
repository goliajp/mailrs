//! `GET /api/storage` — how much of the signed-in user's quota their mail
//! takes, read from the same two keys an import is stopped by. Sending is
//! never stopped, so `used_bytes` can pass `quota_bytes`.

use axum::Json;
use axum::extract::Extension;
use axum::http::StatusCode;
use mailrs_core_sidestate::keys::quota::{limit_key, used_key};

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct Storage {
    pub used_bytes: i64,
    /// `None` when the account has no quota.
    pub quota_bytes: Option<i64>,
}

fn number(v: Option<Vec<u8>>) -> i64 {
    v.and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// A limit of zero or less is no quota, as the import check reads it.
fn storage(limit: i64, used: i64) -> Storage {
    let quota_bytes = match limit > 0 {
        true => Some(limit),
        false => None,
    };
    Storage {
        used_bytes: used,
        quota_bytes,
    }
}

pub async fn get_storage(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Storage>, StatusCode> {
    let (limit, used) = with_kevy(move |c| {
        let limit = number(c.get(limit_key(&user).as_bytes())?);
        let used = number(c.get(used_key(&user).as_bytes())?);
        Ok((limit, used))
    })?;
    Ok(Json(storage(limit, used)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_is_no_quota_and_usage_may_pass_the_quota() {
        assert_eq!(
            storage(0, 5),
            Storage {
                used_bytes: 5,
                quota_bytes: None
            }
        );
        assert_eq!(storage(100, 130).quota_bytes, Some(100));
        assert_eq!(number(Some(b"42".to_vec())), 42);
        assert_eq!(number(None), 0);
    }
}
