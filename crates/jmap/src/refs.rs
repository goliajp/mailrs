//! Resolve back-references between method calls within one JMAP request
//! (RFC 8620 §3.7 "Back-references").
//!
//! A method-call argument shaped like:
//!
//! ```json
//! { "#emailIds": { "resultOf": "callId-1", "name": "Email/query", "path": "/ids" } }
//! ```
//!
//! is rewritten in-place into `{ "emailIds": [...] }` by looking up the
//! response whose call-id matches `resultOf` and pointer-deref'ing `path`.

use serde_json::Value;

/// In-place resolve any back-references (`#key`-prefixed args) inside `args`
/// against the slice of `(method, response, call_id)` tuples produced so far.
pub fn resolve_references(args: &mut Value, previous: &[(String, Value, String)]) {
    let Some(obj) = args.as_object_mut() else {
        return;
    };

    let ref_keys: Vec<String> = obj
        .keys()
        .filter(|k| k.starts_with('#'))
        .cloned()
        .collect();

    for ref_key in ref_keys {
        let Some(ref_val) = obj.remove(&ref_key) else {
            continue;
        };

        let result_of = ref_val.get("resultOf").and_then(|v| v.as_str());
        let name = ref_val.get("name").and_then(|v| v.as_str());
        let path = ref_val.get("path").and_then(|v| v.as_str());

        let (Some(result_of), Some(name), Some(path)) = (result_of, name, path) else {
            continue;
        };

        let resolved = previous
            .iter()
            .find(|(resp_name, _, resp_id)| resp_id == result_of && resp_name == name);

        if let Some((_, resp_value, _)) = resolved
            && let Some(val) = json_pointer(resp_value, path)
        {
            let real_key = ref_key.trim_start_matches('#').to_string();
            obj.insert(real_key, val.clone());
        }
    }
}

/// Tolerant variant of `Value::pointer`: treats an empty pointer or "/" as the
/// root, matching JMAP's spec where omitting `path` returns the entire
/// response object.
pub fn json_pointer<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer == "/" || pointer.is_empty() {
        return Some(value);
    }
    value.pointer(pointer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn previous(call_id: &str, method: &str, body: Value) -> Vec<(String, Value, String)> {
        vec![(method.to_string(), body, call_id.to_string())]
    }

    #[test]
    fn json_pointer_root() {
        let v = serde_json::json!({"a": 1});
        assert_eq!(json_pointer(&v, ""), Some(&v));
        assert_eq!(json_pointer(&v, "/"), Some(&v));
    }

    #[test]
    fn json_pointer_field() {
        let v = serde_json::json!({"ids": [1, 2]});
        assert_eq!(json_pointer(&v, "/ids"), Some(&serde_json::json!([1, 2])));
    }

    #[test]
    fn resolve_back_reference_basic() {
        let prev = previous(
            "c1",
            "Email/query",
            serde_json::json!({"ids": ["msg-1", "msg-2"]}),
        );
        let mut args = serde_json::json!({
            "#ids": {"resultOf": "c1", "name": "Email/query", "path": "/ids"}
        });
        resolve_references(&mut args, &prev);
        assert_eq!(args, serde_json::json!({"ids": ["msg-1", "msg-2"]}));
    }

    #[test]
    fn resolve_skips_missing_call_id() {
        let prev = previous("c1", "Email/query", serde_json::json!({"ids": []}));
        let mut args = serde_json::json!({
            "#ids": {"resultOf": "missing", "name": "Email/query", "path": "/ids"}
        });
        resolve_references(&mut args, &prev);
        // unchanged: ref key removed but no replacement inserted
        assert!(args.get("ids").is_none());
    }

    #[test]
    fn resolve_skips_malformed_reference() {
        let prev = previous("c1", "Email/query", serde_json::json!({"ids": []}));
        let mut args = serde_json::json!({
            "#ids": {"resultOf": "c1"}
        });
        resolve_references(&mut args, &prev);
        assert!(args.get("ids").is_none());
    }

    #[test]
    fn resolve_no_op_on_non_object() {
        let mut args = serde_json::json!([1, 2, 3]);
        resolve_references(&mut args, &[]);
        assert_eq!(args, serde_json::json!([1, 2, 3]));
    }
}
