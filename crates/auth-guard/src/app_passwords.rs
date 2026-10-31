//! Per-device passwords for mail clients, and whether an account's own
//! password still signs in to the mail protocols.
//!
//! ```text
//! app_pw:<account>         hash  id → Stored (JSON)
//! app_pw:used:<account>    hash  id → unix seconds of its last sign-in
//! app_pw:policy:<account>  string  inherit | allow | app-only
//! ```
//!
//! A password is 16 lowercase letters, shown once and kept only as an
//! argon2 hash. The account's own password keeps working on the protocols
//! while the account's policy (or, for `inherit`, the deployment's
//! `MAILRS_PROTOCOL_PASSWORD`) is `allow`.

use std::io;

use kevy_client::Connection;
use rand_core::RngCore;

/// Most passwords one account may hold: a sign-in may compare each.
pub const MAX_PER_ACCOUNT: usize = 20;
/// The letters a password is made of: no `l` or `o`, read as `1` and `0`.
const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
const LEN: usize = 16;
/// A sign-in records its time at most this often.
const USED_EVERY_SECS: i64 = 600;

fn key(account: &str) -> String {
    format!("app_pw:{}", account.to_ascii_lowercase())
}

fn used_key(account: &str) -> String {
    format!("app_pw:used:{}", account.to_ascii_lowercase())
}

fn policy_key(account: &str) -> String {
    format!("app_pw:policy:{}", account.to_ascii_lowercase())
}

/// Whether the account's own password signs in to the mail protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    /// The deployment's default.
    Inherit,
    /// The account password and app passwords.
    Allow,
    /// App passwords only.
    AppOnly,
}

impl Policy {
    /// From its stored or configured name.
    pub fn parse(s: &str) -> Option<Policy> {
        match s.trim() {
            "inherit" => Some(Policy::Inherit),
            "allow" => Some(Policy::Allow),
            "app-only" => Some(Policy::AppOnly),
            _ => None,
        }
    }

    /// Its name.
    pub fn as_str(self) -> &'static str {
        match self {
            Policy::Inherit => "inherit",
            Policy::Allow => "allow",
            Policy::AppOnly => "app-only",
        }
    }
}

/// The deployment's default, `MAILRS_PROTOCOL_PASSWORD`: `allow` when
/// unset.
///
/// # Errors
/// The variable is set to something else than `allow` or `app-only`.
pub fn default_policy() -> Result<Policy, String> {
    match std::env::var("MAILRS_PROTOCOL_PASSWORD") {
        Err(_) => Ok(Policy::Allow),
        Ok(v) => match Policy::parse(&v) {
            Some(p @ (Policy::Allow | Policy::AppOnly)) => Ok(p),
            _ => Err(format!(
                "MAILRS_PROTOCOL_PASSWORD={v:?}: expected allow or app-only"
            )),
        },
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    name: String,
    hash: String,
    created_at: i64,
}

/// One app password, as its owner sees it: never the password.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Info {
    /// Its id.
    pub id: String,
    /// The name its owner gave it.
    pub name: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds of its latest sign-in, to ten minutes.
    pub last_used_at: Option<i64>,
}

/// A new password.
fn generate() -> String {
    let mut bytes = [0u8; LEN];
    rand_core::OsRng.fill_bytes(&mut bytes);
    // 256 % 24 == 16: reject the top values so every letter is as likely
    let mut out = String::with_capacity(LEN);
    let mut pool = bytes.to_vec();
    while out.len() < LEN {
        match pool.pop() {
            Some(b) if (b as usize) < 240 => {
                out.push(ALPHABET[b as usize % ALPHABET.len()] as char)
            }
            Some(_) => {}
            None => {
                let mut more = [0u8; LEN];
                rand_core::OsRng.fill_bytes(&mut more);
                pool.extend_from_slice(&more);
            }
        }
    }
    out
}

/// What a sign-in typed, as a password could be: spaces dropped (the
/// password is shown in groups of four), lowercased. `None` when it
/// cannot be one.
fn normalize(input: &str) -> Option<String> {
    let s: String = input
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    (s.len() == LEN && s.bytes().all(|b| ALPHABET.contains(&b))).then_some(s)
}

/// Why a password was not made.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The account holds [`MAX_PER_ACCOUNT`] already.
    TooMany,
    /// The name is empty or longer than 100 characters.
    BadName,
}

/// Make one named `name` for `account`. Returns its id and the password,
/// which is never shown again.
///
/// # Errors
/// The store could not be read or written, or hashing failed.
pub fn create(
    c: &mut Connection,
    account: &str,
    name: &str,
    now: i64,
) -> io::Result<Result<(String, String), Refused>> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Ok(Err(Refused::BadName));
    }
    if c.hlen(key(account).as_bytes())? >= MAX_PER_ACCOUNT {
        return Ok(Err(Refused::TooMany));
    }
    let secret = generate();
    let hash = crate::password::hash(&secret).map_err(|e| io::Error::other(e.to_string()))?;
    let mut idb = [0u8; 6];
    rand_core::OsRng.fill_bytes(&mut idb);
    let id: String = idb.iter().map(|b| format!("{b:02x}")).collect();
    let stored = Stored {
        name: name.to_string(),
        hash,
        created_at: now,
    };
    let json = serde_json::to_vec(&stored).map_err(io::Error::other)?;
    c.hset(key(account).as_bytes(), &[(id.as_bytes(), json.as_slice())])?;
    Ok(Ok((id, secret)))
}

fn stored(c: &mut Connection, account: &str) -> io::Result<Vec<(String, Stored)>> {
    let flat = c.hgetall(key(account).as_bytes())?;
    Ok(flat
        .chunks_exact(2)
        .filter_map(|p| {
            let id = String::from_utf8_lossy(&p[0]).into_owned();
            match serde_json::from_slice::<Stored>(&p[1]) {
                Ok(s) => Some((id, s)),
                Err(e) => {
                    tracing::warn!(%id, error = %e, "an app password could not be read and is left out");
                    None
                }
            }
        })
        .collect())
}

/// `account`'s passwords, newest first.
///
/// # Errors
/// The store could not be read.
pub fn list(c: &mut Connection, account: &str) -> io::Result<Vec<Info>> {
    let used = c.hgetall(used_key(account).as_bytes())?;
    let last = |id: &str| {
        used.chunks_exact(2)
            .find(|p| p[0] == id.as_bytes())
            .and_then(|p| String::from_utf8_lossy(&p[1]).parse().ok())
    };
    let mut out: Vec<Info> = stored(c, account)?
        .into_iter()
        .map(|(id, s)| Info {
            last_used_at: last(&id),
            id,
            name: s.name,
            created_at: s.created_at,
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Revoke one. Whether it existed.
///
/// # Errors
/// The store could not be written.
pub fn revoke(c: &mut Connection, account: &str, id: &str) -> io::Result<bool> {
    c.hdel(used_key(account).as_bytes(), &[id.as_bytes()])?;
    Ok(c.hdel(key(account).as_bytes(), &[id.as_bytes()])? > 0)
}

/// Revoke every one. How many there were.
///
/// # Errors
/// The store could not be written.
pub fn revoke_all(c: &mut Connection, account: &str) -> io::Result<usize> {
    let n = c.hlen(key(account).as_bytes())?;
    c.del(&[key(account).as_bytes(), used_key(account).as_bytes()])?;
    Ok(n)
}

/// The id of the password `input` is, for `account`; `None` when it is
/// none of them.
///
/// # Errors
/// The store could not be read.
pub fn verify(
    c: &mut Connection,
    account: &str,
    input: &str,
    now: i64,
) -> io::Result<Option<String>> {
    let Some(input) = normalize(input) else {
        return Ok(None);
    };
    let Some(id) = stored(c, account)?
        .into_iter()
        .find(|(_, s)| crate::password::matches(&input, &s.hash))
        .map(|(id, _)| id)
    else {
        return Ok(None);
    };
    let ukey = used_key(account);
    let last: Option<i64> = c
        .hget(ukey.as_bytes(), id.as_bytes())?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok());
    if last.is_none_or(|t| now - t >= USED_EVERY_SECS) {
        c.hset(
            ukey.as_bytes(),
            &[(id.as_bytes(), now.to_string().as_bytes())],
        )?;
    }
    Ok(Some(id))
}

/// `account`'s own setting; `Inherit` when it has none.
///
/// # Errors
/// The store could not be read.
pub fn policy(c: &mut Connection, account: &str) -> io::Result<Policy> {
    Ok(c.get(policy_key(account).as_bytes())?
        .and_then(|v| Policy::parse(&String::from_utf8_lossy(&v)))
        .unwrap_or(Policy::Inherit))
}

/// Set `account`'s setting.
///
/// # Errors
/// The store could not be written.
pub fn set_policy(c: &mut Connection, account: &str, p: Policy) -> io::Result<()> {
    c.set(policy_key(account).as_bytes(), p.as_str().as_bytes())?;
    Ok(())
}

pub use crate::login::ProtocolCheck as Check;

/// Decide a protocol sign-in's password for `account`, with `default`
/// standing in for `inherit`.
///
/// # Errors
/// The store could not be read.
pub fn check(
    c: &mut Connection,
    account: &str,
    input: &str,
    default: Policy,
    now: i64,
) -> io::Result<Check> {
    if let Some(id) = verify(c, account, input, now)? {
        return Ok(Check::AppPassword(id));
    }
    let effective = match policy(c, account)? {
        Policy::Inherit => default,
        p => p,
    };
    Ok(match effective {
        Policy::AppOnly => Check::AppOnly,
        _ => Check::TryAccountPassword,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_sixteen_unambiguous_letters_and_typing_spaces_is_fine() {
        for _ in 0..50 {
            let p = generate();
            assert_eq!(p.len(), 16);
            assert!(p.bytes().all(|b| ALPHABET.contains(&b)), "{p}");
        }
        assert_eq!(
            normalize("abcd efgh ijkm npqr").as_deref(),
            Some("abcdefghijkmnpqr")
        );
        assert_eq!(
            normalize("ABCDEFGHIJKMNPQR").as_deref(),
            Some("abcdefghijkmnpqr")
        );
        assert_eq!(normalize("hello"), None, "too short");
        assert_eq!(normalize("abcdefghijklmnop"), None, "has an l and an o");
    }

    #[test]
    fn made_listed_checked_and_revoked() {
        let mut c = Connection::connect("mem://app-passwords").unwrap();
        let a = "Bob@Golia.jp";
        let (id, secret) = create(&mut c, a, "iPhone Mail", 100).unwrap().unwrap();
        let grouped: String = secret
            .as_bytes()
            .chunks(4)
            .map(|g| std::str::from_utf8(g).unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            verify(&mut c, "bob@golia.jp", &grouped, 200).unwrap(),
            Some(id.clone())
        );
        assert_eq!(verify(&mut c, a, "abcdefghijkmnpqr", 200).unwrap(), None);
        let listed = list(&mut c, a).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].name.as_str(), listed[0].last_used_at),
            ("iPhone Mail", Some(200))
        );
        assert!(
            !format!("{listed:?}").contains(&secret),
            "the password is never listed"
        );
        // a second sign-in inside ten minutes does not move the time
        verify(&mut c, a, &secret, 500).unwrap();
        assert_eq!(list(&mut c, a).unwrap()[0].last_used_at, Some(200));
        assert!(revoke(&mut c, a, &id).unwrap());
        assert_eq!(verify(&mut c, a, &secret, 900).unwrap(), None);
        assert!(!revoke(&mut c, a, &id).unwrap());
    }

    #[test]
    fn names_and_counts_are_bounded() {
        let mut c = Connection::connect("mem://app-passwords-bounds").unwrap();
        assert_eq!(
            create(&mut c, "a@b.jp", "  ", 1).unwrap(),
            Err(Refused::BadName)
        );
        for i in 0..MAX_PER_ACCOUNT {
            create(&mut c, "a@b.jp", &format!("d{i}"), 1)
                .unwrap()
                .unwrap();
        }
        assert_eq!(
            create(&mut c, "a@b.jp", "one more", 1).unwrap(),
            Err(Refused::TooMany)
        );
        assert_eq!(revoke_all(&mut c, "a@b.jp").unwrap(), MAX_PER_ACCOUNT);
        assert!(list(&mut c, "a@b.jp").unwrap().is_empty());
    }

    #[test]
    fn the_policy_decides_whether_the_account_password_is_tried() {
        let mut c = Connection::connect("mem://app-passwords-policy").unwrap();
        let (_, secret) = create(&mut c, "a@b.jp", "laptop", 1).unwrap().unwrap();
        let (id, _) = (list(&mut c, "a@b.jp").unwrap()[0].id.clone(), ());
        assert_eq!(
            check(&mut c, "a@b.jp", &secret, Policy::AppOnly, 1).unwrap(),
            Check::AppPassword(id)
        );
        assert_eq!(
            check(&mut c, "a@b.jp", "pw", Policy::Allow, 1).unwrap(),
            Check::TryAccountPassword
        );
        assert_eq!(
            check(&mut c, "a@b.jp", "pw", Policy::AppOnly, 1).unwrap(),
            Check::AppOnly
        );
        set_policy(&mut c, "a@b.jp", Policy::Allow).unwrap();
        assert_eq!(
            check(&mut c, "a@b.jp", "pw", Policy::AppOnly, 1).unwrap(),
            Check::TryAccountPassword
        );
        set_policy(&mut c, "a@b.jp", Policy::AppOnly).unwrap();
        assert_eq!(
            check(&mut c, "a@b.jp", "pw", Policy::Allow, 1).unwrap(),
            Check::AppOnly
        );
        assert_eq!(Policy::parse("inherit"), Some(Policy::Inherit));
        assert_eq!(Policy::parse("off"), None);
    }
}
