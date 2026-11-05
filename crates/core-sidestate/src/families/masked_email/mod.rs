//! Masked addresses: a random address per site that delivers to its owner,
//! compatible with Fastmail's `MaskedEmail` objects.
//!
//! Two keys per mask, written together by one function and nothing else: the
//! address's own hash, which delivery reads, and a field in its owner's hash,
//! which the owner's list reads. A deleted mask keeps its address hash for
//! ever as a tombstone (state `deleted`, owner kept for abuse handling) and
//! leaves the owner's list: an address that once took a site's mail is never
//! issued again, to anyone.
//!
//! Keys (network kevy):
//!   `mailrs:masked:addr:{address}`   hash, the mask (tombstones included)
//!   `mailrs:masked:user:{owner}`     hash, address → mask JSON (live masks)
//!   `mailrs:masked:day:{owner}:{d}`  counter, masks created that UTC day
//!   `mailrs:masked:submit_log`       list, mail submitted over SMTP from a
//!                                    mask (see [`submit_log`])

use std::io;

use kevy_client::Connection;
use serde::{Deserialize, Serialize};

pub mod submit_log;
mod words;

/// Most live masks one account may hold.
pub const MAX_PER_USER: usize = 1000;
/// Most masks one account may create in a UTC day.
pub const MAX_PER_DAY: i64 = 50;

pub fn addr_key(address: &str) -> String {
    format!("mailrs:masked:addr:{}", address.to_ascii_lowercase())
}

pub fn user_key(owner: &str) -> String {
    format!("mailrs:masked:user:{}", owner.to_ascii_lowercase())
}

/// The log of mail submitted from masks, apart from the admin audit log.
pub const SUBMIT_LOG: &[u8] = b"mailrs:masked:submit_log";

fn day_key(owner: &str, day: i64) -> String {
    format!("mailrs:masked:day:{}:{day}", owner.to_ascii_lowercase())
}

/// Fastmail's states. `pending` takes mail and turns `enabled` when the
/// first message arrives; `disabled` and `deleted` take none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Pending,
    Enabled,
    Disabled,
    Deleted,
}

impl State {
    pub fn parse(s: &str) -> Option<State> {
        match s {
            "pending" => Some(State::Pending),
            "enabled" => Some(State::Enabled),
            "disabled" => Some(State::Disabled),
            "deleted" => Some(State::Deleted),
            _ => None,
        }
    }

    fn takes_mail(self) -> bool {
        matches!(self, State::Pending | State::Enabled)
    }
}

/// One mask, in Fastmail's field names where they have one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mask {
    pub id: String,
    pub email: String,
    pub owner: String,
    pub state: State,
    #[serde(default)]
    pub for_domain: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub url: Option<String>,
    /// Epoch seconds.
    pub created_at: i64,
    /// What created it: `web`, `jmap`, or a key's label.
    #[serde(default)]
    pub created_by: String,
    /// Epoch seconds of the last message delivered to it.
    #[serde(default)]
    pub last_message_at: Option<i64>,
}

/// Why a mask was not created.
#[derive(Debug, PartialEq, Eq)]
pub enum CreateError {
    /// [`MAX_PER_USER`] live masks already.
    TooMany,
    /// [`MAX_PER_DAY`] created today already.
    TooManyToday,
    /// The address is or was a mask.
    Taken,
    /// No free address found; vanishingly unlikely.
    NoFreeAddress,
    Store(String),
}

impl From<io::Error> for CreateError {
    fn from(e: io::Error) -> Self {
        CreateError::Store(e.to_string())
    }
}

impl From<kevy_client::KevyError> for CreateError {
    fn from(e: kevy_client::KevyError) -> Self {
        CreateError::Store(e.to_string())
    }
}

/// What a new mask records besides its address.
#[derive(Debug, Default, Clone)]
pub struct NewMask {
    pub for_domain: String,
    pub description: String,
    pub url: Option<String>,
    pub created_by: String,
    pub state: Option<State>,
}

/// The mask at `address`, tombstones included.
pub fn get(conn: &mut Connection, address: &str) -> io::Result<Option<Mask>> {
    let raw = conn.hget(addr_key(address).as_bytes(), b"json")?;
    Ok(raw.and_then(|b| serde_json::from_slice(&b).ok()))
}

/// Whether `address` is or was a mask: neither may be taken by an account,
/// an alias, a group or another mask.
pub fn is_taken(conn: &mut Connection, address: &str) -> io::Result<bool> {
    Ok(conn.exists(&[addr_key(address).as_bytes()])? > 0)
}

/// `owner`'s live masks, newest first.
pub fn list(conn: &mut Connection, owner: &str) -> io::Result<Vec<Mask>> {
    let flat = conn.hgetall(user_key(owner).as_bytes())?;
    let mut out: Vec<Mask> = flat
        .chunks_exact(2)
        .filter_map(|pair| serde_json::from_slice(&pair[1]).ok())
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.email.cmp(&b.email)));
    Ok(out)
}

/// The addresses `owner` may send from: live masks that take mail.
pub fn sendable(conn: &mut Connection, owner: &str) -> io::Result<Vec<String>> {
    Ok(list(conn, owner)?
        .into_iter()
        .filter(|m| m.state.takes_mail())
        .map(|m| m.email)
        .collect())
}

/// Where mail for `address` goes, when it is a mask: `Some(Some(owner))` to
/// deliver, `Some(None)` to refuse (disabled or deleted), `None` when it is
/// not a mask.
pub fn delivery(conn: &mut Connection, address: &str) -> io::Result<Option<Option<String>>> {
    Ok(get(conn, address)?.map(|m| m.state.takes_mail().then_some(m.owner)))
}

/// Write `m` to its address hash, and to its owner's list unless deleted.
fn save(conn: &mut Connection, m: &Mask) -> io::Result<()> {
    let json = serde_json::to_vec(m).map_err(io::Error::other)?;
    conn.hset(
        addr_key(&m.email).as_bytes(),
        &[(b"json" as &[u8], json.as_slice())],
    )?;
    let user = user_key(&m.owner);
    let field = m.email.to_ascii_lowercase();
    match m.state {
        State::Deleted => {
            conn.hdel(user.as_bytes(), &[field.as_bytes()])?;
        }
        _ => {
            conn.hset(user.as_bytes(), &[(field.as_bytes(), json.as_slice())])?;
        }
    }
    Ok(())
}

/// A fresh address on `domain` from `pick` (a source of random numbers),
/// `<adjective>.<noun>.<dddddd>`.
pub fn candidate(domain: &str, mut pick: impl FnMut(usize) -> usize) -> String {
    let adj = words::ADJECTIVES[pick(words::ADJECTIVES.len())];
    let noun = words::NOUNS[pick(words::NOUNS.len())];
    format!(
        "{adj}.{noun}.{:06}@{}",
        pick(1_000_000),
        domain.to_ascii_lowercase()
    )
}

/// A uniform number below `n` from the OS.
pub fn os_pick(n: usize) -> usize {
    let mut b = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    (u64::from_le_bytes(b) % n as u64) as usize
}

/// Create the mask `email` for `owner`, after the limits: [`MAX_PER_USER`]
/// live masks and [`MAX_PER_DAY`] made today. [`CreateError::Taken`] when
/// the address is or was a mask; the caller checks accounts, aliases and
/// groups, which this store cannot see, and draws another [`candidate`].
pub fn create_at(
    conn: &mut Connection,
    owner: &str,
    email: &str,
    new: &NewMask,
    now: i64,
) -> Result<Mask, CreateError> {
    if conn.hlen(user_key(owner).as_bytes())? >= MAX_PER_USER {
        return Err(CreateError::TooMany);
    }
    let day = day_key(owner, now.div_euclid(86_400));
    let made_today: i64 = conn
        .get(day.as_bytes())?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if made_today >= MAX_PER_DAY {
        return Err(CreateError::TooManyToday);
    }
    if is_taken(conn, email)? {
        return Err(CreateError::Taken);
    }
    let email = email.to_ascii_lowercase();
    let m = Mask {
        id: format!("me-{}", email.split('@').next().unwrap_or_default()),
        owner: owner.to_ascii_lowercase(),
        state: new.state.unwrap_or(State::Enabled),
        for_domain: new.for_domain.clone(),
        description: new.description.clone(),
        url: new.url.clone(),
        created_at: now,
        created_by: new.created_by.clone(),
        last_message_at: None,
        email,
    };
    save(conn, &m)?;
    conn.incr(day.as_bytes())?;
    conn.expire(day.as_bytes(), std::time::Duration::from_secs(2 * 86_400))?;
    Ok(m)
}

/// [`create_at`] on fresh candidates from `pick` until one is free;
/// `taken_elsewhere` answers for accounts, aliases and groups.
pub fn create(
    conn: &mut Connection,
    owner: &str,
    domain: &str,
    new: NewMask,
    now: i64,
    mut pick: impl FnMut(usize) -> usize,
    mut taken_elsewhere: impl FnMut(&str) -> io::Result<bool>,
) -> Result<Mask, CreateError> {
    for _ in 0..TRIES {
        let email = candidate(domain, &mut pick);
        if taken_elsewhere(&email)? {
            continue;
        }
        match create_at(conn, owner, &email, &new, now) {
            Err(CreateError::Taken) => continue,
            done => return done,
        }
    }
    Err(CreateError::NoFreeAddress)
}

/// Candidates drawn before giving up; one collision in 10^8 makes two
/// already rare.
pub const TRIES: usize = 16;

/// What an update may change.
#[derive(Debug, Default, Clone)]
pub struct Patch {
    pub state: Option<State>,
    pub for_domain: Option<String>,
    pub description: Option<String>,
    pub url: Option<Option<String>>,
}

/// Why an update was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// Not `owner`'s, or no such mask.
    NotFound,
    /// A deleted mask stays deleted.
    Deleted,
    Store(String),
}

impl From<io::Error> for UpdateError {
    fn from(e: io::Error) -> Self {
        UpdateError::Store(e.to_string())
    }
}

/// Change `owner`'s mask at `address`. Setting `deleted` makes it a
/// tombstone; nothing brings a tombstone back.
pub fn update(
    conn: &mut Connection,
    owner: &str,
    address: &str,
    patch: Patch,
) -> Result<Mask, UpdateError> {
    let Some(mut m) = get(conn, address)? else {
        return Err(UpdateError::NotFound);
    };
    if !m.owner.eq_ignore_ascii_case(owner) {
        return Err(UpdateError::NotFound);
    }
    if m.state == State::Deleted {
        return Err(UpdateError::Deleted);
    }
    if let Some(s) = patch.state {
        m.state = s;
    }
    if let Some(d) = patch.for_domain {
        m.for_domain = d;
    }
    if let Some(d) = patch.description {
        m.description = d;
    }
    if let Some(u) = patch.url {
        m.url = u;
    }
    save(conn, &m)?;
    Ok(m)
}

/// A message was delivered to `address`: record when, and a pending mask
/// becomes enabled. Nothing for an address that is not a live mask.
pub fn touch(conn: &mut Connection, address: &str, now: i64) -> io::Result<()> {
    let Some(mut m) = get(conn, address)? else {
        return Ok(());
    };
    if !m.state.takes_mail() {
        return Ok(());
    }
    m.last_message_at = Some(now);
    m.state = State::Enabled;
    save(conn, &m)
}

/// A glob over `owner`'s daily creation counters; `owner` already escaped.
pub fn day_glob(owner: &str) -> String {
    format!("mailrs:masked:day:{}:*", owner.to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
