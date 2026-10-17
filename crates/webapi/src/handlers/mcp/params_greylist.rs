//! Parameters of the local greylist tools.

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GreylistLocalAddParams {
    /// Address or domain the rule matches, e.g. `example.com` or
    /// `sender@example.com`.
    pub address_or_domain: String,
    /// `white` (bypass the greylist) or `black` (reject); `whitelist` and
    /// `blacklist` are accepted too.
    pub list_type: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GreylistLocalRemoveParams {
    /// Rule id exactly as `list_greylist_local` returns it: a number, or a
    /// string such as `not-junk:a@b.example` for a rule added by "not junk".
    pub id: RuleId,
}

/// A greylist rule id: numeric for rules added here, a key string for rules
/// the not-junk action wrote.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RuleId {
    Number(i64),
    Key(String),
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Key(k) => f.write_str(k),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GreylistLocalRemoveParams;

    #[test]
    fn a_rule_id_is_a_number_or_a_not_junk_key() {
        let n: GreylistLocalRemoveParams = serde_json::from_str(r#"{"id": 7}"#).unwrap();
        assert_eq!(n.id.to_string(), "7");
        let k: GreylistLocalRemoveParams =
            serde_json::from_str(r#"{"id": "not-junk:a@b.example"}"#).unwrap();
        assert_eq!(k.id.to_string(), "not-junk:a@b.example");
    }
}
