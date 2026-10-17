//! Admin-managed local greylist rules in the network kevy hash
//! `admin:greylist:local-lists`. The receiver reloads the hash every
//! minute (`receiver/src/greylist_local_sync.rs`) and classifies each
//! entry by its value, so `kind` is only for the admin page.
//!
//! The stored vocabulary is the admin page's: `kind` email / domain /
//! cidr, `list` white / black. Two older shapes are still in the hash and
//! are normalised on read: the not-junk path's `{id: 0, address_or_domain,
//! list_type}` keyed `not-junk:{addr}`, and MCP's `address` / `whitelist`
//! / `blacklist` words.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};

use super::{next_id, now_secs};
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

pub(crate) const GL_KEY: &str = "admin:greylist:local-lists";
pub(crate) const GL_CTR: &str = "admin:greylist:local-lists:counter";

/// A local greylist entry as the admin page sends it. The older words
/// (`address`, `whitelist`, `blacklist`) are accepted and stored as the
/// page's.
#[derive(Debug, serde::Deserialize)]
pub struct CreateGreylistRequest {
    /// `email`, `domain` or `cidr` — what `value` is.
    pub kind: String,
    /// `white` or `black`.
    pub list: String,
    pub value: String,
    /// Free-text reason. `null` and `""` both mean none; the UI sends
    /// `null` for an untouched field.
    #[serde(default)]
    pub note: Option<String>,
}

pub(crate) fn normalise_list(list: &str) -> Option<&'static str> {
    match list {
        "white" | "whitelist" => Some("white"),
        "black" | "blacklist" => Some("black"),
        _ => None,
    }
}

pub(crate) fn normalise_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "email" | "address" => Some("email"),
        "domain" => Some("domain"),
        "cidr" => Some("cidr"),
        _ => None,
    }
}

/// The kind a bare value reads as, for rows and callers that carry none.
pub(crate) fn kind_of_value(value: &str) -> &'static str {
    if value.contains('@') {
        return "email";
    }
    if value.contains('/') {
        return "cidr";
    }
    "domain"
}

pub(crate) struct NewGreylistEntry<'a> {
    pub kind: &'static str,
    pub list: &'static str,
    pub value: &'a str,
    pub note: Option<&'a str>,
    pub created_by: &'a str,
}

/// Write one entry in the current shape under a fresh numeric id.
pub(crate) fn insert_entry(
    c: &mut kevy_client::Connection,
    e: &NewGreylistEntry<'_>,
) -> std::io::Result<serde_json::Value> {
    let id = next_id(c, GL_CTR)?;
    let row = serde_json::json!({
        "id": id,
        "kind": e.kind,
        "list": e.list,
        "value": e.value,
        "note": e.note.filter(|n| !n.is_empty()),
        "created_by": e.created_by,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&row).map_err(std::io::Error::other)?;
    c.hset(
        GL_KEY.as_bytes(),
        &[(id.to_string().as_bytes(), payload.as_slice())],
    )?;
    Ok(row)
}

/// One stored row in the page's shape. `id` is the hash field: a number
/// for current rows, the field string (`not-junk:{addr}`) for the old
/// not-junk rows, whose stored `id` is always 0.
pub(crate) fn normalise_row(field: &[u8], raw: &[u8]) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let text = |a: &str, b: &str| {
        v.get(a)
            .or_else(|| v.get(b))
            .and_then(|x| x.as_str())
            .map(str::to_string)
    };
    let value = text("value", "address_or_domain")?;
    // same rule as the receiver: anything not white is enforced as black
    let list = text("list", "list_type")
        .as_deref()
        .and_then(normalise_list)
        .unwrap_or("black");
    let kind = text("kind", "kind")
        .as_deref()
        .and_then(normalise_kind)
        .unwrap_or_else(|| kind_of_value(&value));
    let field = String::from_utf8_lossy(field).into_owned();
    let id = field
        .parse::<i64>()
        .map(serde_json::Value::from)
        .unwrap_or_else(|_| serde_json::Value::String(field));
    Some(serde_json::json!({
        "id": id,
        "kind": kind,
        "list": list,
        "value": value,
        "note": v.get("note").cloned().unwrap_or(serde_json::Value::Null),
        "created_by": v.get("created_by").cloned().unwrap_or(serde_json::Value::Null),
        "created_at": v.get("created_at").and_then(|x| x.as_i64()).unwrap_or(0),
    }))
}

pub(crate) fn list_entries(
    c: &mut kevy_client::Connection,
) -> std::io::Result<Vec<serde_json::Value>> {
    let flat = c.hgetall(GL_KEY.as_bytes())?;
    let mut items: Vec<serde_json::Value> = flat
        .chunks(2)
        .filter_map(|pair| match pair {
            [field, raw] => normalise_row(field, raw),
            _ => None,
        })
        .collect();
    items.sort_by_key(|r| std::cmp::Reverse(r["created_at"].as_i64().unwrap_or(0)));
    Ok(items)
}

/// Add `addr` to the white list unless a white entry for it exists
/// already, in either stored shape. Returns whether it wrote.
pub(crate) fn ensure_white_entry(
    c: &mut kevy_client::Connection,
    addr: &str,
    created_by: &str,
) -> std::io::Result<bool> {
    let present = list_entries(c)?.iter().any(|r| {
        r["list"] == "white"
            && r["value"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(addr))
    });
    if present {
        return Ok(false);
    }
    insert_entry(
        c,
        &NewGreylistEntry {
            kind: kind_of_value(addr),
            list: "white",
            value: addr,
            note: Some("marked not junk"),
            created_by,
        },
    )?;
    Ok(true)
}

pub async fn list_greylist_local() -> Result<Json<serde_json::Value>, StatusCode> {
    let items = with_kevy(list_entries)?;
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn create_greylist_entry(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateGreylistRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let kind = normalise_kind(&req.kind).ok_or(StatusCode::BAD_REQUEST)?;
    let list = normalise_list(&req.list).ok_or(StatusCode::BAD_REQUEST)?;
    let value = req.value.trim();
    if value.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let row = with_kevy(|c| {
        insert_entry(
            c,
            &NewGreylistEntry {
                kind,
                list,
                value,
                note: req.note.as_deref(),
                created_by: &user,
            },
        )
    })?;
    Ok(Json(row))
}

/// Remove the entry stored under hash field `id`; false when there was none.
pub(crate) fn delete_entry(c: &mut kevy_client::Connection, id: &str) -> std::io::Result<bool> {
    Ok(c.hdel(GL_KEY.as_bytes(), &[id.as_bytes()])? > 0)
}

/// `id` is the hash field the list returned, numeric or not.
pub async fn delete_greylist_entry(Path(id): Path<String>) -> Result<StatusCode, StatusCode> {
    if !with_kevy(move |c| delete_entry(c, &id))? {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> kevy_client::Connection {
        kevy_client::Connection::connect(&format!("mem://{name}")).unwrap()
    }

    #[test]
    fn old_not_junk_rows_and_mcp_words_read_in_the_page_vocabulary() {
        let mut c = conn("gl-normalise");
        let legacy = br#"{"id":0,"address_or_domain":"a@b.c","list_type":"white","created_at":5}"#;
        let mcp = br#"{"id":4,"kind":"address","list":"blacklist","value":"x@y.z","note":null,"created_at":9}"#;
        c.hset(
            GL_KEY.as_bytes(),
            &[
                (b"not-junk:a@b.c" as &[u8], legacy.as_slice()),
                (b"4", mcp.as_slice()),
            ],
        )
        .unwrap();
        let rows = list_entries(&mut c).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], 4);
        assert_eq!(
            (rows[0]["kind"].as_str(), rows[0]["list"].as_str()),
            (Some("email"), Some("black"))
        );
        assert_eq!(rows[1]["id"], "not-junk:a@b.c");
        assert_eq!(rows[1]["value"], "a@b.c");
        // the id the list hands out is the one delete takes
        assert!(delete_entry(&mut c, "not-junk:a@b.c").unwrap());
        assert!(!delete_entry(&mut c, "not-junk:a@b.c").unwrap());
        assert!(!delete_entry(&mut c, "0").unwrap());
        assert_eq!(
            (rows[1]["kind"].as_str(), rows[1]["list"].as_str()),
            (Some("email"), Some("white"))
        );
    }

    #[test]
    fn ensure_white_writes_the_current_shape_once_and_sees_old_rows() {
        let mut c = conn("gl-ensure");
        assert!(ensure_white_entry(&mut c, "new@b.c", "u@b.c").unwrap());
        assert!(!ensure_white_entry(&mut c, "NEW@b.c", "u@b.c").unwrap());
        let legacy =
            br#"{"id":0,"address_or_domain":"old@b.c","list_type":"white","created_at":1}"#;
        c.hset(
            GL_KEY.as_bytes(),
            &[(b"not-junk:old@b.c" as &[u8], legacy.as_slice())],
        )
        .unwrap();
        assert!(!ensure_white_entry(&mut c, "old@b.c", "u@b.c").unwrap());
        let rows = list_entries(&mut c).unwrap();
        assert_eq!(rows.len(), 2);
        let new = rows.iter().find(|r| r["value"] == "new@b.c").unwrap();
        assert!(new["id"].is_i64());
        assert_eq!(new["kind"], "email");
        assert_eq!(new["list"], "white");
        assert_eq!(new["created_by"], "u@b.c");
    }

    #[test]
    fn vocabulary_accepts_old_words_and_rejects_unknown_ones() {
        assert_eq!(normalise_list("whitelist"), Some("white"));
        assert_eq!(normalise_list("black"), Some("black"));
        assert_eq!(normalise_list("grey"), None);
        assert_eq!(normalise_kind("address"), Some("email"));
        assert_eq!(normalise_kind("ip"), None);
        assert_eq!(kind_of_value("10.0.0.0/8"), "cidr");
        assert_eq!(kind_of_value("example.com"), "domain");
    }
}
