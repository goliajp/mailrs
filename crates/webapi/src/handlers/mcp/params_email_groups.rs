//! Parameters of the email group member tools.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmailGroupMemberParams {
    /// Email group id as `create_email_group` and `list_email_groups`
    /// return it.
    #[serde(deserialize_with = "group_id")]
    #[schemars(with = "i64")]
    pub group_id: i64,
    /// Member address to add / remove.
    pub address: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmailGroupIdParams {
    /// Email group id as `create_email_group` and `list_email_groups`
    /// return it.
    #[serde(deserialize_with = "group_id")]
    #[schemars(with = "i64")]
    pub id: i64,
}

/// A group id is a number. These tools took it as a string until
/// 2026-11-05, so a string of digits is still read.
fn group_id<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Sent {
        Number(i64),
        Text(String),
    }
    match Sent::deserialize(d)? {
        Sent::Number(n) => Ok(n),
        Sent::Text(s) => s
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("group id {s:?} is not a number"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_id_is_a_number_or_its_digits() {
        let n: EmailGroupMemberParams =
            serde_json::from_str(r#"{"group_id": 7, "address": "a@x.test"}"#).unwrap();
        assert_eq!(n.group_id, 7);
        let s: EmailGroupMemberParams =
            serde_json::from_str(r#"{"group_id": "7", "address": "a@x.test"}"#).unwrap();
        assert_eq!(s.group_id, 7);
        let id: EmailGroupIdParams = serde_json::from_str(r#"{"id": 7}"#).unwrap();
        assert_eq!(id.id, 7);
        assert!(
            serde_json::from_str::<EmailGroupMemberParams>(
                r#"{"group_id": "desk", "address": "a@x.test"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn the_schema_says_number() {
        let schema = serde_json::to_value(schemars::schema_for!(EmailGroupMemberParams)).unwrap();
        assert_eq!(schema["properties"]["group_id"]["type"], "integer");
        let schema = serde_json::to_value(schemars::schema_for!(EmailGroupIdParams)).unwrap();
        assert_eq!(schema["properties"]["id"]["type"], "integer");
    }
}
