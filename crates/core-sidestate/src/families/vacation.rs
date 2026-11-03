//! A user's vacation reply, set from the web.
//!
//! ```text
//! vacation:{user}   JSON {enabled, subject, body, start, end, days, version}
//! ```
//!
//! Read by the spool drain of either core; the reply itself is RFC 5230's,
//! sent by `mailrs_intake::sieve::maybe_vacation_reply`, with every rule
//! about whom never to answer.

use serde::{Deserialize, Serialize};

/// Longest subject, in characters.
pub const SUBJECT_MAX: usize = 200;
/// Longest body, in characters.
pub const BODY_MAX: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vacation {
    pub enabled: bool,
    pub subject: String,
    pub body: String,
    /// Unix seconds from which it replies, or none.
    pub start: Option<i64>,
    /// Unix seconds until which it replies, or none.
    pub end: Option<i64>,
    /// One reply per sender within this many days, 1 to 30.
    pub days: u32,
    /// Raised on every save, so a changed reply goes out again to senders
    /// the old one already answered.
    #[serde(default)]
    pub version: u64,
}

/// Why a setting was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Subject,
    Body,
    Days,
    Window,
}

impl Refused {
    /// The error code the API answers with.
    pub fn code(self) -> &'static str {
        match self {
            Refused::Subject => "invalid_subject",
            Refused::Body => "invalid_body",
            Refused::Days => "invalid_days",
            Refused::Window => "end_before_start",
        }
    }
}

impl Vacation {
    /// Whether it replies to mail arriving at `now`.
    pub fn active_at(&self, now: i64) -> bool {
        self.enabled && self.start.is_none_or(|s| now >= s) && self.end.is_none_or(|e| now < e)
    }

    fn check(&self) -> Result<(), Refused> {
        // it becomes a header line of the reply as it is
        if self.subject.chars().count() > SUBJECT_MAX || self.subject.contains(['\r', '\n']) {
            return Err(Refused::Subject);
        }
        if (self.enabled && self.body.trim().is_empty()) || self.body.chars().count() > BODY_MAX {
            return Err(Refused::Body);
        }
        if !(1..=30).contains(&self.days) {
            return Err(Refused::Days);
        }
        if let (Some(s), Some(e)) = (self.start, self.end)
            && e <= s
        {
            return Err(Refused::Window);
        }
        Ok(())
    }
}

/// Key of `user`'s vacation reply.
#[must_use]
pub fn key(user: &str) -> String {
    format!("vacation:{}", user.to_lowercase())
}

/// `user`'s vacation reply, if one was ever set.
///
/// # Errors
/// The store could not be read.
pub fn get(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Option<Vacation>> {
    let Some(bytes) = conn.get(key(user).as_bytes())? else {
        return Ok(None);
    };
    Ok(serde_json::from_slice(&bytes).ok())
}

/// Store `user`'s vacation reply, raising its version.
///
/// # Errors
/// The store could not be read or written; a refusal is the inner `Err`.
pub fn set(
    conn: &mut kevy_client::Connection,
    user: &str,
    mut v: Vacation,
) -> std::io::Result<Result<Vacation, Refused>> {
    if let Err(why) = v.check() {
        return Ok(Err(why));
    }
    v.version = get(conn, user)?.map_or(1, |old| old.version + 1);
    let bytes = serde_json::to_vec(&v).map_err(std::io::Error::other)?;
    conn.set(key(user).as_bytes(), &bytes)?;
    Ok(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn away() -> Vacation {
        Vacation {
            enabled: true,
            subject: "Away".into(),
            body: "Back on Monday.".into(),
            start: Some(100),
            end: Some(200),
            days: 7,
            version: 0,
        }
    }

    #[test]
    fn it_replies_only_inside_its_window() {
        let v = away();
        assert!(!v.active_at(99));
        assert!(v.active_at(100));
        assert!(v.active_at(199));
        assert!(!v.active_at(200));
        let open = Vacation {
            start: None,
            end: None,
            ..away()
        };
        assert!(open.active_at(0));
        let off = Vacation {
            enabled: false,
            ..open
        };
        assert!(!off.active_at(0));
    }

    /// The subject becomes a header line of the reply as it is.
    #[test]
    fn a_subject_with_a_line_break_is_refused() {
        for subject in ["Away\r\nBcc: x@evil.test", "Away\nX: y", "Away\rX: y"] {
            let v = Vacation {
                subject: subject.into(),
                ..away()
            };
            assert_eq!(v.check(), Err(Refused::Subject), "{subject:?}");
        }
    }

    #[test]
    fn saving_checks_and_raises_the_version() {
        let mut c = kevy_client::Connection::connect("mem://vacation-set").unwrap();
        assert_eq!(
            set(&mut c, "u@x.test", Vacation { days: 0, ..away() }).unwrap(),
            Err(Refused::Days)
        );
        assert_eq!(
            set(
                &mut c,
                "u@x.test",
                Vacation {
                    body: " ".into(),
                    ..away()
                }
            )
            .unwrap(),
            Err(Refused::Body)
        );
        assert_eq!(
            set(
                &mut c,
                "u@x.test",
                Vacation {
                    end: Some(50),
                    ..away()
                }
            )
            .unwrap(),
            Err(Refused::Window)
        );
        assert_eq!(set(&mut c, "U@x.test", away()).unwrap().unwrap().version, 1);
        assert_eq!(set(&mut c, "u@x.test", away()).unwrap().unwrap().version, 2);
        assert_eq!(get(&mut c, "u@x.test").unwrap().unwrap().version, 2);
        assert_eq!(get(&mut c, "other@x.test").unwrap(), None);
    }
}
