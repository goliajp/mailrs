//! Bitmask <-> JMAP keyword conversions (RFC 8621 §4.1).

use serde_json::{Map, Value};

use crate::types::{FLAG_ANSWERED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_SEEN};

/// Render a flag bitmask into a JMAP `keywords` object.
pub fn flags_to_keywords(flags: u32) -> Value {
    let mut kw = Map::new();
    if flags & FLAG_SEEN != 0 {
        kw.insert("$seen".to_string(), Value::Bool(true));
    }
    if flags & FLAG_ANSWERED != 0 {
        kw.insert("$answered".to_string(), Value::Bool(true));
    }
    if flags & FLAG_FLAGGED != 0 {
        kw.insert("$flagged".to_string(), Value::Bool(true));
    }
    if flags & FLAG_DRAFT != 0 {
        kw.insert("$draft".to_string(), Value::Bool(true));
    }
    Value::Object(kw)
}

/// Parse a JMAP `keywords` object back into a flag bitmask. Non-object input,
/// unknown keywords, and `false` values are silently ignored.
pub fn keywords_to_flags(keywords: &Value) -> u32 {
    let Some(obj) = keywords.as_object() else {
        return 0;
    };
    let mut flags = 0u32;
    for (k, v) in obj {
        if v.as_bool() != Some(true) {
            continue;
        }
        flags |= keyword_to_flag(k);
    }
    flags
}

/// Map a single JMAP keyword name to its flag bit. Unknown keywords return 0.
pub fn keyword_to_flag(keyword: &str) -> u32 {
    match keyword {
        "$seen" => FLAG_SEEN,
        "$answered" => FLAG_ANSWERED,
        "$flagged" => FLAG_FLAGGED,
        "$draft" => FLAG_DRAFT,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bitmask_yields_empty_object() {
        assert_eq!(flags_to_keywords(0), serde_json::json!({}));
    }

    #[test]
    fn all_known_flags_round_trip() {
        let bits = FLAG_SEEN | FLAG_ANSWERED | FLAG_FLAGGED | FLAG_DRAFT;
        let kw = flags_to_keywords(bits);
        assert_eq!(keywords_to_flags(&kw), bits);
    }

    #[test]
    fn unknown_keyword_ignored() {
        let kw = serde_json::json!({"$junk": true, "$seen": true});
        assert_eq!(keywords_to_flags(&kw), FLAG_SEEN);
    }

    #[test]
    fn keyword_false_does_not_set() {
        let kw = serde_json::json!({"$seen": false});
        assert_eq!(keywords_to_flags(&kw), 0);
    }

    #[test]
    fn non_object_keywords_yields_zero() {
        assert_eq!(keywords_to_flags(&Value::Null), 0);
        assert_eq!(keywords_to_flags(&serde_json::json!([1, 2])), 0);
    }

    #[test]
    fn keyword_to_flag_known() {
        assert_eq!(keyword_to_flag("$seen"), FLAG_SEEN);
        assert_eq!(keyword_to_flag("$answered"), FLAG_ANSWERED);
        assert_eq!(keyword_to_flag("$flagged"), FLAG_FLAGGED);
        assert_eq!(keyword_to_flag("$draft"), FLAG_DRAFT);
    }

    #[test]
    fn keyword_to_flag_unknown_is_zero() {
        assert_eq!(keyword_to_flag("$other"), 0);
        assert_eq!(keyword_to_flag(""), 0);
    }
}
