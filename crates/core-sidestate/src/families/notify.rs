//! When to push "you have mail", per reader, and how often it happened.
//!
//! ```text
//! notify:{user}                      hash  mode, quiet_start, quiet_end, utc_offset_minutes
//! notify:{user}:senders              hash  address -> always | never
//! notify:{user}:threads              hash  thread id -> follow | mute
//! push:stats:{user}:{day}:{channel}:{outcome}   counter, kept 90 days
//! ```
//!
//! Preferences, edited in place; the counters are derived and expire.

use serde::{Deserialize, Serialize};

mod site;
pub use site::{ChannelTotals, SiteDay, site_stats};

pub fn prefs_key(user: &str) -> String {
    format!("notify:{}", user.to_lowercase())
}
pub fn senders_key(user: &str) -> String {
    format!("notify:{}:senders", user.to_lowercase())
}
pub fn threads_key(user: &str) -> String {
    format!("notify:{}:threads", user.to_lowercase())
}

/// Which mail pushes, before the per-sender and per-thread choices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Mail to answer, and mail from important contacts.
    #[default]
    Default,
    /// Every message that is not Junk, held, a feed or the reader's own.
    All,
    /// Important contacts only.
    Important,
    Off,
}

/// A reader's push preferences.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub mode: Mode,
    /// Quiet hours as minutes after local midnight; both set or neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_start: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_end: Option<u16>,
    /// The reader's offset from UTC, for the quiet hours.
    #[serde(default)]
    pub utc_offset_minutes: i16,
    /// Say only "new mail": no sender or subject on a lock screen.
    #[serde(default)]
    pub hide_content: bool,
}

impl Prefs {
    /// Whether `now` (epoch seconds) falls in the quiet hours. A window
    /// that ends before it starts runs over midnight.
    pub fn quiet_at(&self, now: i64) -> bool {
        let (Some(start), Some(end)) = (self.quiet_start, self.quiet_end) else {
            return false;
        };
        let local = (now / 60 + i64::from(self.utc_offset_minutes)).rem_euclid(24 * 60) as u16;
        match start <= end {
            true => local >= start && local < end,
            false => local >= start || local < end,
        }
    }
}

pub fn prefs(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Prefs> {
    let raw = conn.hget(prefs_key(user).as_bytes(), b"json")?;
    Ok(raw
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default())
}

pub fn put_prefs(conn: &mut kevy_client::Connection, user: &str, p: &Prefs) -> std::io::Result<()> {
    let body = serde_json::to_vec(p).map_err(std::io::Error::other)?;
    conn.hset(
        prefs_key(user).as_bytes(),
        &[(b"json" as &[u8], body.as_slice())],
    )?;
    Ok(())
}

/// One per-sender or per-thread choice; `None` removes it.
pub fn set_override(
    conn: &mut kevy_client::Connection,
    key: &str,
    field: &str,
    value: Option<&str>,
) -> std::io::Result<()> {
    let field = field.to_ascii_lowercase();
    match value {
        Some(v) => {
            conn.hset(key.as_bytes(), &[(field.as_bytes(), v.as_bytes())])?;
        }
        None => {
            conn.hdel(key.as_bytes(), &[field.as_bytes()])?;
        }
    }
    Ok(())
}

pub fn get_override(
    conn: &mut kevy_client::Connection,
    key: &str,
    field: &str,
) -> std::io::Result<Option<String>> {
    Ok(conn
        .hget(key.as_bytes(), field.to_ascii_lowercase().as_bytes())?
        .map(|v| String::from_utf8_lossy(&v).into_owned()))
}

/// Every override under `key`, as `(field, value)`.
pub fn overrides(
    conn: &mut kevy_client::Connection,
    key: &str,
) -> std::io::Result<Vec<(String, String)>> {
    let flat = conn.hgetall(key.as_bytes())?;
    let mut out: Vec<(String, String)> = flat
        .chunks_exact(2)
        .map(|p| {
            (
                String::from_utf8_lossy(&p[0]).into_owned(),
                String::from_utf8_lossy(&p[1]).into_owned(),
            )
        })
        .collect();
    out.sort();
    Ok(out)
}

/// A browser's Web Push subscription as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSubscription {
    pub endpoint: String,
    /// The browser's `p256dh` key, base64url.
    pub p256dh: String,
    /// The browser's `auth` secret, base64url.
    pub auth: String,
    #[serde(default)]
    pub created_at: i64,
}

/// The field an endpoint is filed under: its sha256, hex.
pub fn endpoint_field(endpoint: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(endpoint.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Keep `sub` for `user`; the same endpoint replaces its old keys.
pub fn add_web_subscription(
    conn: &mut kevy_client::Connection,
    user: &str,
    sub: &WebSubscription,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(sub).map_err(std::io::Error::other)?;
    let key = crate::keys::push::web_subscriptions_key(user);
    conn.hset(
        key.as_bytes(),
        &[(endpoint_field(&sub.endpoint).as_bytes(), body.as_slice())],
    )?;
    Ok(())
}

/// Forget `endpoint`; whether it was there.
pub fn remove_web_subscription(
    conn: &mut kevy_client::Connection,
    user: &str,
    endpoint: &str,
) -> std::io::Result<bool> {
    let key = crate::keys::push::web_subscriptions_key(user);
    Ok(conn.hdel(key.as_bytes(), &[endpoint_field(endpoint).as_bytes()])? > 0)
}

pub fn web_subscriptions(
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<Vec<WebSubscription>> {
    let key = crate::keys::push::web_subscriptions_key(user);
    Ok(conn
        .hgetall(key.as_bytes())?
        .chunks_exact(2)
        .filter_map(|p| serde_json::from_slice(&p[1]).ok())
        .collect())
}

/// The channels and outcomes the counters are kept for.
pub const CHANNELS: [&str; 3] = ["apns", "web", "unifiedpush"];
pub const OUTCOMES: [&str; 5] = [
    "sent",
    "suppressed_policy",
    "suppressed_quiet",
    "failed",
    "clicked",
];

/// How long a day's counters are kept.
const STATS_DAYS: u64 = 90;

fn stats_key(user: &str, day: &str, channel: &str, outcome: &str) -> String {
    format!(
        "push:stats:{}:{day}:{channel}:{outcome}",
        user.to_lowercase()
    )
}

/// The UTC day of `epoch`, `YYYYMMDD`.
pub fn day_of(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0)
        .unwrap_or_default()
        .format("%Y%m%d")
        .to_string()
}

/// Count one push decision.
pub fn count(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: &str,
    outcome: &str,
    now: i64,
) -> std::io::Result<()> {
    let key = stats_key(user, &day_of(now), channel, outcome);
    if conn.incr(key.as_bytes())? == 1 {
        conn.expire(
            key.as_bytes(),
            std::time::Duration::from_secs(STATS_DAYS * 86_400),
        )?;
    }
    Ok(())
}

/// One day's counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DayStats {
    pub day: String,
    /// `channel -> outcome -> count`, zero counts left out.
    pub counts: std::collections::BTreeMap<String, std::collections::BTreeMap<String, i64>>,
}

/// The last `days` days of `user`'s counters, newest first.
pub fn stats(
    conn: &mut kevy_client::Connection,
    user: &str,
    days: u32,
    now: i64,
) -> std::io::Result<Vec<DayStats>> {
    let mut out = Vec::new();
    for back in 0..i64::from(days) {
        let day = day_of(now - back * 86_400);
        let mut s = DayStats {
            day: day.clone(),
            ..Default::default()
        };
        for channel in CHANNELS {
            for outcome in OUTCOMES {
                let n: i64 = conn
                    .get(stats_key(user, &day, channel, outcome).as_bytes())?
                    .and_then(|v| String::from_utf8_lossy(&v).parse().ok())
                    .unwrap_or(0);
                if n > 0 {
                    s.counts
                        .entry(channel.to_string())
                        .or_default()
                        .insert(outcome.to_string(), n);
                }
            }
        }
        out.push(s);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_respect_the_offset_and_midnight() {
        // 22:00–07:00 at UTC+9: 13:00 UTC is 22:00 local
        let p = Prefs {
            quiet_start: Some(22 * 60),
            quiet_end: Some(7 * 60),
            utc_offset_minutes: 9 * 60,
            ..Default::default()
        };
        let at = |h: i64| h * 3600;
        assert!(p.quiet_at(at(13)), "22:00 local");
        assert!(p.quiet_at(at(21)), "06:00 local");
        assert!(!p.quiet_at(at(22)), "07:00 local");
        assert!(!p.quiet_at(at(3)), "12:00 local");
        assert!(!Prefs::default().quiet_at(at(13)), "no window, never quiet");
    }

    #[test]
    fn counters_and_overrides_round_trip() {
        let mut c = kevy_client::Connection::connect("mem://notify-counters").unwrap();
        let now = 1_793_500_000;
        count(&mut c, "A@x", "apns", "sent", now).unwrap();
        count(&mut c, "a@x", "apns", "sent", now).unwrap();
        count(&mut c, "a@x", "apns", "suppressed_policy", now).unwrap();
        let s = stats(&mut c, "a@x", 2, now).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].counts["apns"]["sent"], 2);
        assert_eq!(s[0].counts["apns"]["suppressed_policy"], 1);
        assert!(s[1].counts.is_empty());

        let k = senders_key("a@x");
        set_override(&mut c, &k, "Boss@X", Some("always")).unwrap();
        assert_eq!(
            get_override(&mut c, &k, "boss@x").unwrap().as_deref(),
            Some("always")
        );
        set_override(&mut c, &k, "boss@x", None).unwrap();
        assert_eq!(get_override(&mut c, &k, "boss@x").unwrap(), None);

        let p = Prefs {
            mode: Mode::Important,
            hide_content: true,
            ..Default::default()
        };
        put_prefs(&mut c, "a@x", &p).unwrap();
        assert_eq!(prefs(&mut c, "a@x").unwrap(), p);

        let sub = WebSubscription {
            endpoint: "https://fcm.googleapis.com/fcm/send/1".into(),
            p256dh: "k".into(),
            auth: "a".into(),
            created_at: 1,
        };
        add_web_subscription(&mut c, "a@x", &sub).unwrap();
        add_web_subscription(
            &mut c,
            "a@x",
            &WebSubscription {
                p256dh: "k2".into(),
                ..sub.clone()
            },
        )
        .unwrap();
        let subs = web_subscriptions(&mut c, "a@x").unwrap();
        assert_eq!(subs.len(), 1, "one endpoint, one subscription");
        assert_eq!(subs[0].p256dh, "k2");
        assert!(remove_web_subscription(&mut c, "a@x", &sub.endpoint).unwrap());
        assert!(web_subscriptions(&mut c, "a@x").unwrap().is_empty());
    }
}
