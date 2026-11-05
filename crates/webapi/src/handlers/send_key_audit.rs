//! The audit line a send made with an agent key leaves.

use axum::http::{HeaderMap, StatusCode};

use crate::session::AgentKey;

/// A send made with an agent key leaves an audit line before anything is
/// filed or queued; when the line cannot be written, nothing is sent.
pub(crate) fn record_key_send(
    key: Option<&AgentKey>,
    user: &str,
    headers: &HeaderMap,
    message_id: &str,
) -> Result<(), StatusCode> {
    let Some(key) = key else {
        return Ok(());
    };
    crate::handlers::audit::record_required(
        user,
        "mail.send.agent_key",
        message_id,
        &key_send_detail(key, headers),
    )
}

/// `x-caller` is what the caller says it is, so it is cut short and kept
/// to printable characters.
fn key_send_detail(key: &AgentKey, headers: &HeaderMap) -> String {
    let caller: String = headers
        .get("x-caller")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    format!("key={} caller={caller} ip={}", key.prefix, key.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audit_line_names_the_key_caller_and_source() {
        let key = AgentKey {
            id: 3,
            prefix: "mk_ab12c".into(),
            source: "203.0.113.9".parse().unwrap(),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-caller", "insight weekly\treport".parse().unwrap());
        assert_eq!(
            key_send_detail(&key, &headers),
            "key=mk_ab12c caller=insight weeklyreport ip=203.0.113.9"
        );
        headers.insert("x-caller", "a".repeat(100).parse().unwrap());
        assert_eq!(
            key_send_detail(&key, &headers),
            format!("key=mk_ab12c caller={} ip=203.0.113.9", "a".repeat(64))
        );
        assert_eq!(
            key_send_detail(&key, &HeaderMap::new()),
            "key=mk_ab12c caller= ip=203.0.113.9"
        );
    }
}
