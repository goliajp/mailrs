//! Audit log.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRowWire {
    pub id: i64,
    /// webapi's recorder and fastcore's system events write the fact
    /// time as `occurred_at`; older rows carry `timestamp`
    #[serde(alias = "occurred_at")]
    pub timestamp: i64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditListResponse {
    pub items: Vec<AuditRowWire>,
}

#[cfg(test)]
mod tests {
    use super::AuditRowWire;

    #[test]
    fn audit_rows_read_in_both_stored_shapes() {
        let recorded = br#"{"id":7,"occurred_at":1790000000,"recorded_at":1790000001,"actor":"system","action":"mail.bounce","target":"a@b.c","detail":"DSN delivered to sender"}"#;
        let legacy = br#"{"id":3,"timestamp":1780000000,"actor":"x@y.z","action":"auth.login","target":"x@y.z","detail":""}"#;
        let r: AuditRowWire = serde_json::from_slice(recorded).unwrap();
        assert_eq!(
            (r.id, r.timestamp, r.action.as_str()),
            (7, 1_790_000_000, "mail.bounce")
        );
        let l: AuditRowWire = serde_json::from_slice(legacy).unwrap();
        assert_eq!((l.id, l.timestamp), (3, 1_780_000_000));
        let out = serde_json::to_value(&r).unwrap();
        assert_eq!(out["timestamp"], 1_790_000_000);
        assert!(out.get("occurred_at").is_none());
    }
}
