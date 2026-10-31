//! A user's forwarding to an address elsewhere, set from the web.
//!
//! ```text
//! forwarding:{user}   JSON {address, keep_copy, verified_at,
//!                           pending_address, code_hash, code_expires_at, attempts}
//! ```
//!
//! An address forwards only once its owner has typed back the code sent to
//! it: a stolen session cannot quietly send someone's mail to itself, and
//! nobody's mail can be pointed at a stranger who never asked for it.
//! Read by the spool drain of either core.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long a code can be typed back.
pub const CODE_TTL_SECS: i64 = 24 * 3600;
/// Wrong codes before the pending address is dropped.
pub const ATTEMPTS_MAX: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forwarding {
    /// Where mail goes, once verified; empty when off.
    #[serde(default)]
    pub address: String,
    /// Keep a copy here as well.
    #[serde(default = "yes")]
    pub keep_copy: bool,
    #[serde(default)]
    pub verified_at: i64,
    /// Waiting for its code.
    #[serde(default)]
    pub pending_address: String,
    #[serde(default)]
    pub pending_keep_copy: bool,
    #[serde(default)]
    code_hash: String,
    #[serde(default)]
    code_expires_at: i64,
    #[serde(default)]
    attempts: u32,
}

fn yes() -> bool {
    true
}

/// Off, and set to keep a copy when it is turned on, as a stored one with
/// no `keep_copy` reads.
impl Default for Forwarding {
    fn default() -> Self {
        Self {
            address: String::new(),
            keep_copy: true,
            verified_at: 0,
            pending_address: String::new(),
            pending_keep_copy: true,
            code_hash: String::new(),
            code_expires_at: 0,
            attempts: 0,
        }
    }
}

impl Forwarding {
    /// The verified address mail goes to, if any.
    pub fn target(&self) -> Option<&str> {
        match self.address.is_empty() {
            true => None,
            false => Some(&self.address),
        }
    }
}

/// What typing a code back did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verified {
    /// Forwarding to this address is on.
    On(String),
    /// Wrong code; this many tries left.
    Wrong(u32),
    /// No address waits for a code, it expired, or too many wrong codes.
    Nothing,
}

/// Key of `user`'s forwarding.
#[must_use]
pub fn key(user: &str) -> String {
    format!("forwarding:{}", user.to_lowercase())
}

fn hash(code: &str) -> String {
    hex(&Sha256::digest(code.trim().as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `user`'s forwarding as stored, or the default (off).
///
/// # Errors
/// The store could not be read.
pub fn get(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Forwarding> {
    let Some(bytes) = conn.get(key(user).as_bytes())? else {
        return Ok(Forwarding::default());
    };
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

fn put(conn: &mut kevy_client::Connection, user: &str, f: &Forwarding) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(f).map_err(std::io::Error::other)?;
    conn.set(key(user).as_bytes(), &bytes)?;
    Ok(())
}

/// Ask to forward to `address`: returns the six-digit code to send there.
/// What forwards now keeps forwarding until the code comes back.
///
/// # Errors
/// The store could not be read or written.
pub fn request(
    conn: &mut kevy_client::Connection,
    user: &str,
    address: &str,
    keep_copy: bool,
    now: i64,
) -> std::io::Result<String> {
    let mut bytes = [0u8; 4];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    let code = format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000);
    let mut f = get(conn, user)?;
    f.pending_address = address.trim().to_lowercase();
    f.pending_keep_copy = keep_copy;
    f.code_hash = hash(&code);
    f.code_expires_at = now + CODE_TTL_SECS;
    f.attempts = 0;
    put(conn, user, &f)?;
    Ok(code)
}

/// Type back the code sent to the pending address.
///
/// # Errors
/// The store could not be read or written.
pub fn verify(
    conn: &mut kevy_client::Connection,
    user: &str,
    code: &str,
    now: i64,
) -> std::io::Result<Verified> {
    let mut f = get(conn, user)?;
    if f.pending_address.is_empty() || now >= f.code_expires_at {
        return Ok(Verified::Nothing);
    }
    if hash(code) != f.code_hash {
        f.attempts += 1;
        let left = ATTEMPTS_MAX.saturating_sub(f.attempts);
        if left == 0 {
            f.pending_address.clear();
            f.code_hash.clear();
        }
        put(conn, user, &f)?;
        return Ok(match left {
            0 => Verified::Nothing,
            n => Verified::Wrong(n),
        });
    }
    f.address = std::mem::take(&mut f.pending_address);
    f.keep_copy = f.pending_keep_copy;
    f.verified_at = now;
    f.code_hash.clear();
    put(conn, user, &f)?;
    Ok(Verified::On(f.address))
}

/// Stop forwarding, and forget any address waiting for its code.
///
/// # Errors
/// The store could not be written.
pub fn off(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<()> {
    conn.del(&[key(user).as_bytes()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_forwards_only_after_its_code_comes_back() {
        let mut c = kevy_client::Connection::connect("mem://forwarding-verify").unwrap();
        let code = request(&mut c, "U@x.test", "Me@Elsewhere.example", false, 1000).unwrap();
        assert_eq!(code.len(), 6);
        assert_eq!(get(&mut c, "u@x.test").unwrap().target(), None);
        let wrong = match code.as_str() {
            "000000" => "111111",
            _ => "000000",
        };
        assert_eq!(
            verify(&mut c, "u@x.test", wrong, 1001).unwrap(),
            Verified::Wrong(4)
        );
        assert_eq!(
            verify(&mut c, "u@x.test", &code, 1002).unwrap(),
            Verified::On("me@elsewhere.example".into())
        );
        let f = get(&mut c, "u@x.test").unwrap();
        assert_eq!(
            (f.target(), f.keep_copy),
            (Some("me@elsewhere.example"), false)
        );
        // used once
        assert_eq!(
            verify(&mut c, "u@x.test", &code, 1003).unwrap(),
            Verified::Nothing
        );
        off(&mut c, "u@x.test").unwrap();
        assert_eq!(get(&mut c, "u@x.test").unwrap().target(), None);
    }

    #[test]
    fn a_code_expires_and_wrong_codes_run_out() {
        let mut c = kevy_client::Connection::connect("mem://forwarding-expiry").unwrap();
        let code = request(&mut c, "u@x.test", "a@b.example", true, 0).unwrap();
        assert_eq!(
            verify(&mut c, "u@x.test", &code, CODE_TTL_SECS).unwrap(),
            Verified::Nothing
        );
        let code = request(&mut c, "u@x.test", "a@b.example", true, 0).unwrap();
        let wrong = match code.as_str() {
            "000000" => "111111",
            _ => "000000",
        };
        for left in (1..ATTEMPTS_MAX).rev() {
            assert_eq!(
                verify(&mut c, "u@x.test", wrong, 1).unwrap(),
                Verified::Wrong(left)
            );
        }
        assert_eq!(
            verify(&mut c, "u@x.test", wrong, 1).unwrap(),
            Verified::Nothing
        );
        // the right code no longer helps
        assert_eq!(
            verify(&mut c, "u@x.test", &code, 2).unwrap(),
            Verified::Nothing
        );
    }

    #[test]
    fn never_set_reads_as_off_keeping_a_copy() {
        let mut c = kevy_client::Connection::connect("mem://forwarding-default").unwrap();
        let f = get(&mut c, "u@x.test").unwrap();
        assert_eq!((f.target(), f.keep_copy), (None, true));
    }

    #[test]
    fn a_new_request_keeps_the_verified_address_forwarding() {
        let mut c = kevy_client::Connection::connect("mem://forwarding-keep").unwrap();
        let code = request(&mut c, "u@x.test", "old@b.example", true, 0).unwrap();
        verify(&mut c, "u@x.test", &code, 1).unwrap();
        request(&mut c, "u@x.test", "new@b.example", true, 2).unwrap();
        assert_eq!(
            get(&mut c, "u@x.test").unwrap().target(),
            Some("old@b.example")
        );
    }
}
