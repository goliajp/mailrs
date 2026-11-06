//! Params for the ops daemon tools (batch 19).

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpsHistoryParams {
    /// Unix seconds; events from then on. The last day when absent.
    #[serde(default)]
    pub since: Option<i64>,
    /// At most this many rows of each kind (default 200, at most 1000).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpsSilenceParams {
    /// The rule's name, as `get_ops_state` lists it under `rules`.
    pub rule: String,
    /// One object of the rule (as an alert shows it); every object when absent.
    #[serde(default)]
    pub object: Option<String>,
    /// How long the silence lasts, in seconds; required and positive.
    pub for_secs: i64,
    /// Why, recorded with the silence.
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpsEndSilenceParams {
    /// The silence's id, from `get_ops_state` or `set_ops_silence`.
    pub id: String,
    /// Why it ends early, recorded.
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpsMaintenanceParams {
    /// `start`, `extend` or `end`.
    pub action: String,
    /// How long from now maintenance lasts, in seconds; required to start
    /// or extend.
    #[serde(default)]
    pub for_secs: Option<i64>,
    /// Why, recorded with the change.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silence_must_say_how_long_and_why() {
        let schema = serde_json::to_value(schemars::schema_for!(OpsSilenceParams)).unwrap();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for field in ["rule", "for_secs", "reason"] {
            assert!(required.contains(&field), "{field}: {schema}");
        }
        assert!(!required.contains(&"object"));
        let p: OpsMaintenanceParams =
            serde_json::from_str(r#"{"action": "end", "reason": "done"}"#).unwrap();
        assert_eq!((p.action.as_str(), p.for_secs), ("end", None));
    }
}
