//! Creation ids across the method calls of one request (RFC 8620 §5.3).
//!
//! A `/set` call names each object it creates with a creation id; a later
//! call in the same request may write `#` + that id wherever an id goes,
//! as RFC 8621 does when `EmailSubmission/set` sends the draft an
//! `Email/set` just created (`"emailId": "#k1490"`). Only places that hold
//! ids are rewritten, never free text, so a subject that happens to read
//! `#k1490` stays as written.

use serde_json::{Map, Value};
use std::collections::HashMap;

/// The ids created so far in a request, by creation id.
#[derive(Debug, Default)]
pub struct CreatedIds(HashMap<String, String>);

impl CreatedIds {
    /// Take the `created` map of a `/set` response.
    pub fn record(&mut self, method: &str, response: &Value) {
        if !method.ends_with("/set") {
            return;
        }
        let Some(created) = response.get("created").and_then(Value::as_object) else {
            return;
        };
        for (cid, object) in created {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                self.0.insert(cid.clone(), id.to_string());
            }
        }
    }

    /// Rewrite `#cid` in the id-holding places of a call's arguments.
    pub fn resolve(&self, args: &mut Value) {
        if self.0.is_empty() {
            return;
        }
        if let Some(obj) = args.as_object_mut() {
            self.object(obj);
        }
    }

    fn id(&self, s: &str) -> Option<String> {
        self.0.get(s.strip_prefix('#')?).cloned()
    }

    fn string(&self, v: &mut Value) {
        if let Some(id) = v.as_str().and_then(|s| self.id(s)) {
            *v = Value::String(id);
        }
    }

    fn strings(&self, v: &mut Value) {
        match v {
            Value::Array(items) => items.iter_mut().for_each(|i| self.string(i)),
            Value::Object(map) => *map = self.keys(std::mem::take(map)),
            _ => {}
        }
    }

    // a map keyed by ids, as `mailboxIds` and `update` are
    fn keys(&self, map: Map<String, Value>) -> Map<String, Value> {
        map.into_iter()
            .map(|(k, v)| (self.id(&k).unwrap_or(k), v))
            .collect()
    }

    // a patch path such as `mailboxIds/#m`: the segment after an id map
    fn patch_key(&self, k: String) -> String {
        match k.split_once("Ids/") {
            Some((head, tail)) => match self.id(tail) {
                Some(id) => format!("{head}Ids/{id}"),
                None => k,
            },
            None => k,
        }
    }

    fn object(&self, obj: &mut Map<String, Value>) {
        let taken = std::mem::take(obj);
        for (k, mut v) in taken {
            match k.as_str() {
                // creation ids being defined, and submission's own
                // references to the submissions this same call creates
                "onSuccessUpdateEmail" | "onSuccessDestroyEmail" => {}
                "create" => {
                    if let Some(entries) = v.as_object_mut() {
                        entries.values_mut().for_each(|e| self.nested(e));
                    }
                }
                "update" => {
                    if let Some(entries) = v.as_object_mut() {
                        let mut renamed = self.keys(std::mem::take(entries));
                        renamed.values_mut().for_each(|e| self.nested(e));
                        *entries = renamed;
                    }
                }
                "ids" | "destroy" => self.strings(&mut v),
                _ if k.ends_with("Ids") => self.strings(&mut v),
                _ if k == "id" || k.ends_with("Id") => self.string(&mut v),
                _ => self.nested(&mut v),
            }
            obj.insert(self.patch_key(k), v);
        }
    }

    fn nested(&self, v: &mut Value) {
        match v {
            Value::Object(obj) => self.object(obj),
            Value::Array(items) => items.iter_mut().for_each(|i| self.nested(i)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn after_email_set() -> CreatedIds {
        let mut c = CreatedIds::default();
        c.record(
            "Email/set",
            &json!({ "created": { "k1490": { "id": "M42", "threadId": "T1" } } }),
        );
        c.record(
            "Mailbox/set",
            &json!({ "created": { "m": { "id": "box9" } } }),
        );
        // a /get answer names nothing created
        c.record("Email/get", &json!({ "created": { "x": { "id": "M1" } } }));
        c
    }

    #[test]
    fn a_submission_sends_the_draft_created_earlier_in_the_request() {
        let mut args = json!({
            "accountId": "a",
            "create": { "s": { "emailId": "#k1490", "identityId": "I1" } },
            "onSuccessUpdateEmail": { "#s": { "keywords/$draft": null } }
        });
        after_email_set().resolve(&mut args);
        assert_eq!(args["create"]["s"]["emailId"], "M42");
        assert_eq!(args["create"]["s"]["identityId"], "I1");
        // the submission's own creation ids are left for it to resolve
        assert!(args["onSuccessUpdateEmail"].get("#s").is_some());
    }

    #[test]
    fn ids_in_lists_maps_and_patches_are_rewritten() {
        let mut args = json!({
            "ids": ["#k1490", "M7"],
            "destroy": ["#k1490"],
            "create": { "n": { "mailboxIds": { "#m": true }, "subject": "#k1490" } },
            "update": { "#k1490": { "mailboxIds/#m": true, "keywords/$seen": true } }
        });
        after_email_set().resolve(&mut args);
        assert_eq!(args["ids"], json!(["M42", "M7"]));
        assert_eq!(args["destroy"], json!(["M42"]));
        assert_eq!(args["create"]["n"]["mailboxIds"], json!({ "box9": true }));
        // free text is not an id
        assert_eq!(args["create"]["n"]["subject"], "#k1490");
        assert_eq!(
            args["update"]["M42"],
            json!({ "mailboxIds/box9": true, "keywords/$seen": true })
        );
    }

    #[test]
    fn an_unknown_creation_id_and_a_get_are_left_alone() {
        let c = after_email_set();
        let mut args = json!({ "create": { "s": { "emailId": "#nope" } }, "ids": ["#x"] });
        c.resolve(&mut args);
        assert_eq!(args["create"]["s"]["emailId"], "#nope");
        assert_eq!(args["ids"], json!(["#x"]));
    }
}
