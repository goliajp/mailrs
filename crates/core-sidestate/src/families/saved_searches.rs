//! A user's saved searches.
//!
//! ```text
//! saved_searches:{user}       hash  id → {"name","query","created_at"}
//! saved_searches:{user}:seq   counter the ids are taken from
//! ```
//!
//! A setting, not mail: the query is the text a person typed, kept to run
//! again. Written and read by the webapi only.

use serde::{Deserialize, Serialize};

/// Longest name, in characters.
pub const NAME_MAX: usize = 64;
/// Longest query, in characters.
pub const QUERY_MAX: usize = 500;
/// Most searches one user keeps.
pub const COUNT_MAX: usize = 100;

/// One saved search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSearch {
    pub id: String,
    pub name: String,
    pub query: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// Why a search was not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Empty after trimming, or longer than [`NAME_MAX`].
    Name,
    /// Empty after trimming, or longer than [`QUERY_MAX`].
    Query,
    /// Another saved search has this name, ignoring case.
    Duplicate,
    /// Already [`COUNT_MAX`].
    TooMany,
}

impl Refused {
    /// The error code the API answers with.
    pub fn code(self) -> &'static str {
        match self {
            Refused::Name => "invalid_name",
            Refused::Query => "invalid_query",
            Refused::Duplicate => "name_taken",
            Refused::TooMany => "too_many",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    name: String,
    query: String,
    created_at: i64,
}

/// Key of `user`'s saved searches.
#[must_use]
pub fn key(user: &str) -> String {
    format!("saved_searches:{}", user.to_lowercase())
}

fn seq_key(user: &str) -> String {
    format!("{}:seq", key(user))
}

/// `user`'s saved searches, by name.
///
/// # Errors
/// The store could not be read.
pub fn list(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<SavedSearch>> {
    let flat = conn.hgetall(key(user).as_bytes())?;
    // field, value, field, value, ...
    let mut out: Vec<SavedSearch> = flat
        .chunks_exact(2)
        .filter_map(|pair| {
            let id = String::from_utf8(pair[0].clone()).ok()?;
            let s: Stored = serde_json::from_slice(&pair[1]).ok()?;
            Some(SavedSearch {
                id,
                name: s.name,
                query: s.query,
                created_at: s.created_at,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

/// Save a search under `name`.
///
/// # Errors
/// The store could not be read or written; a refusal is the inner `Err`.
pub fn create(
    conn: &mut kevy_client::Connection,
    user: &str,
    name: &str,
    query: &str,
    now: i64,
) -> std::io::Result<Result<SavedSearch, Refused>> {
    let (name, query) = (name.trim(), query.trim());
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Ok(Err(Refused::Name));
    }
    if query.is_empty() || query.chars().count() > QUERY_MAX {
        return Ok(Err(Refused::Query));
    }
    let existing = list(conn, user)?;
    if existing.len() >= COUNT_MAX {
        return Ok(Err(Refused::TooMany));
    }
    if existing
        .iter()
        .any(|s| s.name.to_lowercase() == name.to_lowercase())
    {
        return Ok(Err(Refused::Duplicate));
    }
    let id = conn.incr(seq_key(user).as_bytes())?.to_string();
    let stored = Stored {
        name: name.to_string(),
        query: query.to_string(),
        created_at: now,
    };
    let bytes = serde_json::to_vec(&stored).map_err(std::io::Error::other)?;
    conn.hset(key(user).as_bytes(), &[(id.as_bytes(), bytes.as_slice())])?;
    Ok(Ok(SavedSearch {
        id,
        name: stored.name,
        query: stored.query,
        created_at: now,
    }))
}

/// Remove one saved search. Returns whether it existed.
///
/// # Errors
/// The store could not be written.
pub fn delete(conn: &mut kevy_client::Connection, user: &str, id: &str) -> std::io::Result<bool> {
    Ok(conn.hdel(key(user).as_bytes(), &[id.as_bytes()])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_list_and_delete() {
        let mut c = kevy_client::Connection::connect("mem://saved-searches-roundtrip").unwrap();
        let a = create(
            &mut c,
            "U@x.test",
            " Invoices ",
            "has:attachment invoice",
            10,
        )
        .unwrap()
        .unwrap();
        assert_eq!(a.name, "Invoices");
        create(&mut c, "u@x.test", "alpha", "from:a", 11)
            .unwrap()
            .unwrap();
        let names: Vec<String> = list(&mut c, "u@x.test")
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["alpha", "Invoices"]);
        assert!(list(&mut c, "other@x.test").unwrap().is_empty());
        assert!(delete(&mut c, "u@x.test", &a.id).unwrap());
        assert!(!delete(&mut c, "u@x.test", &a.id).unwrap());
        assert_eq!(list(&mut c, "u@x.test").unwrap().len(), 1);
    }

    #[test]
    fn what_is_refused() {
        let mut c = kevy_client::Connection::connect("mem://saved-searches-refused").unwrap();
        let mut try_save =
            |name: &str, query: &str| create(&mut c, "u@x.test", name, query, 1).unwrap().err();
        assert_eq!(try_save("  ", "x"), Some(Refused::Name));
        assert_eq!(
            try_save(&"n".repeat(NAME_MAX + 1), "x"),
            Some(Refused::Name)
        );
        assert_eq!(try_save("ok", " "), Some(Refused::Query));
        assert_eq!(
            try_save("ok", &"q".repeat(QUERY_MAX + 1)),
            Some(Refused::Query)
        );
        assert_eq!(try_save("Same", "x"), None);
        assert_eq!(try_save("same", "y"), Some(Refused::Duplicate));
    }

    #[test]
    fn a_user_keeps_at_most_count_max() {
        let mut c = kevy_client::Connection::connect("mem://saved-searches-cap").unwrap();
        for i in 0..COUNT_MAX {
            create(&mut c, "u@x.test", &format!("s{i}"), "x", 1)
                .unwrap()
                .unwrap();
        }
        let over = create(&mut c, "u@x.test", "one more", "x", 1).unwrap();
        assert_eq!(over, Err(Refused::TooMany));
    }
}
