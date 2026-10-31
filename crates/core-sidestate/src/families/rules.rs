//! A user's rules, and the record of each run over existing mail.
//!
//! ```text
//! rules:{user}              hash  id → {"name","query","actions","enabled","created_at"}
//! rules:{user}:seq          counter the rule ids are taken from
//! rules:{user}:run:{run}    what one run changed, to undo it; kept 7 days
//! rules:{user}:runseq       counter the run ids are taken from
//! ```
//!
//! Written by the webapi; read by both cores as mail arrives. The checks a
//! rule passes before it is stored are `mailrs_mailbox::rules::check`;
//! this family stores what it is given.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Longest name, in characters.
pub const NAME_MAX: usize = 64;
/// Most rules one user keeps.
pub const COUNT_MAX: usize = 100;
/// How long a run can be undone.
pub const RUN_KEPT: Duration = Duration::from_secs(7 * 24 * 3600);

/// One rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub query: String,
    /// Stored spellings, as `mailrs_mailbox::rules::Action::spelling`.
    pub actions: Vec<String>,
    pub enabled: bool,
    /// Unix seconds.
    pub created_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    name: String,
    query: String,
    actions: Vec<String>,
    enabled: bool,
    created_at: i64,
}

/// Why a rule was not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Empty after trimming, or longer than [`NAME_MAX`].
    Name,
    /// Another rule has this name, ignoring case.
    Duplicate,
    /// Already [`COUNT_MAX`].
    TooMany,
    /// No rule with this id.
    NotFound,
}

impl Refused {
    /// The error code the API answers with.
    pub fn code(self) -> &'static str {
        match self {
            Refused::Name => "invalid_name",
            Refused::Duplicate => "name_taken",
            Refused::TooMany => "too_many",
            Refused::NotFound => "not_found",
        }
    }
}

/// Key of `user`'s rules.
#[must_use]
pub fn key(user: &str) -> String {
    format!("rules:{}", user.to_lowercase())
}

fn run_key(user: &str, run: &str) -> String {
    format!("{}:run:{run}", key(user))
}

/// `user`'s rules, oldest first: the order they run in.
///
/// # Errors
/// The store could not be read.
pub fn list(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<Rule>> {
    let flat = conn.hgetall(key(user).as_bytes())?;
    // field, value, field, value, ...
    let mut out: Vec<Rule> = flat
        .chunks_exact(2)
        .filter_map(|pair| {
            let id = String::from_utf8(pair[0].clone()).ok()?;
            let s: Stored = serde_json::from_slice(&pair[1]).ok()?;
            Some(Rule {
                id,
                name: s.name,
                query: s.query,
                actions: s.actions,
                enabled: s.enabled,
                created_at: s.created_at,
            })
        })
        .collect();
    out.sort_by_key(|r| (r.id.parse::<u64>().unwrap_or(u64::MAX), r.id.clone()));
    Ok(out)
}

/// What a rule is made of, checked by the caller.
#[derive(Debug, Clone)]
pub struct Draft<'a> {
    pub name: &'a str,
    pub query: &'a str,
    pub actions: Vec<String>,
    pub enabled: bool,
}

/// Store a new rule.
///
/// # Errors
/// The store could not be read or written; a refusal is the inner `Err`.
pub fn create(
    conn: &mut kevy_client::Connection,
    user: &str,
    draft: Draft<'_>,
    now: i64,
) -> std::io::Result<Result<Rule, Refused>> {
    let rules = list(conn, user)?;
    if rules.len() >= COUNT_MAX {
        return Ok(Err(Refused::TooMany));
    }
    let id = conn
        .incr(format!("{}:seq", key(user)).as_bytes())?
        .to_string();
    put(conn, user, &rules, id, draft, now)
}

/// Replace a rule's name, query, actions and switch.
///
/// # Errors
/// The store could not be read or written; a refusal is the inner `Err`.
pub fn update(
    conn: &mut kevy_client::Connection,
    user: &str,
    id: &str,
    draft: Draft<'_>,
) -> std::io::Result<Result<Rule, Refused>> {
    let rules = list(conn, user)?;
    let Some(old) = rules.iter().find(|r| r.id == id) else {
        return Ok(Err(Refused::NotFound));
    };
    let created_at = old.created_at;
    let others: Vec<Rule> = rules.iter().filter(|r| r.id != id).cloned().collect();
    put(conn, user, &others, id.to_string(), draft, created_at)
}

fn put(
    conn: &mut kevy_client::Connection,
    user: &str,
    others: &[Rule],
    id: String,
    draft: Draft<'_>,
    created_at: i64,
) -> std::io::Result<Result<Rule, Refused>> {
    let name = draft.name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Ok(Err(Refused::Name));
    }
    if others
        .iter()
        .any(|r| r.name.to_lowercase() == name.to_lowercase())
    {
        return Ok(Err(Refused::Duplicate));
    }
    let stored = Stored {
        name: name.to_string(),
        query: draft.query.trim().to_string(),
        actions: draft.actions,
        enabled: draft.enabled,
        created_at,
    };
    let bytes = serde_json::to_vec(&stored).map_err(std::io::Error::other)?;
    conn.hset(key(user).as_bytes(), &[(id.as_bytes(), bytes.as_slice())])?;
    Ok(Ok(Rule {
        id,
        name: stored.name,
        query: stored.query,
        actions: stored.actions,
        enabled: stored.enabled,
        created_at,
    }))
}

/// Remove a rule. Returns whether it existed.
///
/// # Errors
/// The store could not be written.
pub fn delete(conn: &mut kevy_client::Connection, user: &str, id: &str) -> std::io::Result<bool> {
    Ok(conn.hdel(key(user).as_bytes(), &[id.as_bytes()])? > 0)
}

/// How one conversation stood before a run changed it: what undoing the
/// run puts back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Before {
    pub thread_id: String,
    pub archived: bool,
    pub unread: bool,
    pub starred: bool,
    pub junk: bool,
    pub trashed: bool,
    /// Label ids it already carried, of those the run puts on.
    pub labels: Vec<String>,
}

/// One run of a rule over existing mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub rule_id: String,
    pub actions: Vec<String>,
    pub before: Vec<Before>,
}

/// Keep a run for [`RUN_KEPT`]; returns its id.
///
/// # Errors
/// The store could not be written.
pub fn record_run(
    conn: &mut kevy_client::Connection,
    user: &str,
    run: &Run,
) -> std::io::Result<String> {
    let id = conn
        .incr(format!("{}:runseq", key(user)).as_bytes())?
        .to_string();
    let bytes = serde_json::to_vec(run).map_err(std::io::Error::other)?;
    conn.set_with_ttl(run_key(user, &id).as_bytes(), &bytes, RUN_KEPT)?;
    Ok(id)
}

/// Take a run out of the record, so it is undone once.
///
/// # Errors
/// The store could not be read or written.
pub fn take_run(
    conn: &mut kevy_client::Connection,
    user: &str,
    id: &str,
) -> std::io::Result<Option<Run>> {
    let key = run_key(user, id);
    let Some(bytes) = conn.get(key.as_bytes())? else {
        return Ok(None);
    };
    conn.del(&[key.as_bytes()])?;
    Ok(serde_json::from_slice(&bytes).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str) -> Draft<'_> {
        Draft {
            name,
            query: "from:shop.example",
            actions: vec!["archive".into()],
            enabled: true,
        }
    }

    #[test]
    fn rules_are_kept_in_the_order_made() {
        let mut c = kevy_client::Connection::connect("mem://rules-order").unwrap();
        for n in ["b", "a", "c"] {
            create(&mut c, "U@x.test", draft(n), 1).unwrap().unwrap();
        }
        let names: Vec<String> = list(&mut c, "u@x.test")
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["b", "a", "c"]);
    }

    #[test]
    fn names_are_checked_on_create_and_update() {
        let mut c = kevy_client::Connection::connect("mem://rules-names").unwrap();
        let shop = create(&mut c, "u@x.test", draft("Shop"), 1)
            .unwrap()
            .unwrap();
        create(&mut c, "u@x.test", draft("Bank"), 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            create(&mut c, "u@x.test", draft("shop"), 1).unwrap(),
            Err(Refused::Duplicate)
        );
        assert_eq!(
            create(&mut c, "u@x.test", draft(" "), 1).unwrap(),
            Err(Refused::Name)
        );
        // keeping its own name is not a clash; taking another's is
        let same = update(&mut c, "u@x.test", &shop.id, draft("SHOP")).unwrap();
        assert_eq!(same.map(|r| r.name), Ok("SHOP".to_string()));
        assert_eq!(
            update(&mut c, "u@x.test", &shop.id, draft("bank")).unwrap(),
            Err(Refused::Duplicate)
        );
        assert_eq!(
            update(&mut c, "u@x.test", "99", draft("x")).unwrap(),
            Err(Refused::NotFound)
        );
        assert!(delete(&mut c, "u@x.test", &shop.id).unwrap());
        assert!(!delete(&mut c, "u@x.test", &shop.id).unwrap());
    }

    #[test]
    fn a_run_is_undone_once() {
        let mut c = kevy_client::Connection::connect("mem://rules-runs").unwrap();
        let run = Run {
            rule_id: "1".into(),
            actions: vec!["archive".into()],
            before: vec![Before {
                thread_id: "t".into(),
                archived: false,
                unread: true,
                starred: false,
                junk: false,
                trashed: false,
                labels: vec![],
            }],
        };
        let id = record_run(&mut c, "u@x.test", &run).unwrap();
        assert_eq!(take_run(&mut c, "u@x.test", &id).unwrap(), Some(run));
        assert_eq!(take_run(&mut c, "u@x.test", &id).unwrap(), None);
        assert_eq!(take_run(&mut c, "other@x.test", &id).unwrap(), None);
    }
}
