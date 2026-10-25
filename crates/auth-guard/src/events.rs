//! Login events: one fact per sign-in attempt, in the format every product
//! shares, so the same question ("who tried this account, from where, how")
//! has the same answer everywhere.
//!
//! Facts are only appended. They are kept per day (`auth:events:YYYYMMDD`,
//! newest first) for [`RETAIN_DAYS`], and a day keeps at most [`DAY_CAP`]:
//! past that, attempts are only counted, so a flood cannot push the store
//! over. Ways in that authenticate on every request (DAV's Basic header,
//! agent keys) record each failure but only the first success per account
//! or key and source each day.

use std::io;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// The product a fact is about.
pub const PRODUCT: &str = "mailrs";
/// Days a fact is kept.
pub const RETAIN_DAYS: u64 = 90;
/// Facts kept per day.
pub const DAY_CAP: u64 = 200_000;

/// Where the attempt came in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// The web client's login form.
    Web,
    /// A credential check made for another service.
    Verify,
    /// IMAP.
    Imap,
    /// POP3.
    Pop3,
    /// SMTP submission.
    Smtp,
    /// ManageSieve.
    Managesieve,
    /// CalDAV and CardDAV.
    Dav,
    /// An API or agent key.
    ApiKey,
    /// An external identity provider.
    Oidc,
}

/// What was presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Credential {
    /// The account password.
    Password,
    /// A one-time code.
    Totp,
    /// A passkey.
    Passkey,
    /// A recovery code.
    RecoveryCode,
    /// A per-device password for mail clients.
    AppPassword,
    /// An API or agent key.
    ApiKey,
    /// A session token.
    Session,
    /// A link sent by mail.
    EmailLink,
    /// An external identity.
    External,
}

/// How it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Signed in.
    Ok,
    /// Wrong credentials.
    BadPassword,
    /// No such account.
    NoAccount,
    /// Refused by the login guard without checking.
    Locked,
    /// Right password; a second factor is still to come.
    MfaRequired,
    /// The second factor was wrong.
    MfaFailed,
    /// The source is not allowed on this listener.
    DeniedByAllowlist,
    /// The account is disabled.
    Disabled,
}

/// One sign-in attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginEvent {
    /// Unix seconds.
    pub occurred_at: u64,
    /// Which product the attempt was made on.
    pub product: String,
    /// As typed when it is an address, otherwise only its length: a
    /// password typed into the user field must not end up here.
    pub account: String,
    /// The client, by the shared rule (a trusted proxy's last hop, else the
    /// connection).
    pub source_ip: IpAddr,
    /// Where it came in.
    pub channel: Channel,
    /// What was presented.
    pub credential: Credential,
    /// How it ended.
    pub result: Outcome,
    /// Web only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// The key's id, never the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
}

impl LoginEvent {
    /// An attempt happening now.
    pub fn new(
        channel: Channel,
        credential: Credential,
        source: IpAddr,
        account: &str,
        result: Outcome,
    ) -> Self {
        Self {
            occurred_at: crate::login::unix_now(),
            product: PRODUCT.into(),
            // no name at all (a key that resolves to nobody) stays empty
            account: match account.trim().is_empty() {
                true => String::new(),
                false => crate::login::loggable_account(account),
            },
            source_ip: source.to_canonical(),
            channel,
            credential,
            result,
            user_agent: None,
            key_id: None,
        }
    }

    /// The browser's user agent, cut to 256 characters.
    pub fn with_user_agent(mut self, user_agent: Option<&str>) -> Self {
        self.user_agent = user_agent.map(|ua| ua.chars().take(256).collect());
        self
    }

    /// The key's id (never the key itself).
    pub fn with_key(mut self, key_id: &str) -> Self {
        self.key_id = Some(key_id.to_string());
        self
    }

    /// The structured log line every fact also gets, so a container log
    /// answers the same questions while the store is out of reach.
    pub fn log(&self) {
        let (channel, credential, result) = (
            name(&self.channel),
            name(&self.credential),
            name(&self.result),
        );
        let key = self.key_id.as_deref().unwrap_or("");
        match self.result {
            Outcome::Ok => {
                tracing::info!(event = "auth", channel, credential, source = %self.source_ip, account = %self.account, result, key)
            }
            _ => {
                tracing::warn!(event = "auth", channel, credential, source = %self.source_ip, account = %self.account, result, key)
            }
        }
    }
}

fn name<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What an admin asks the facts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Query {
    /// This account, compared without case.
    #[serde(default)]
    pub account: Option<String>,
    /// This source address.
    #[serde(default)]
    pub source: Option<IpAddr>,
    /// This outcome.
    #[serde(default)]
    pub result: Option<Outcome>,
    /// This way in.
    #[serde(default)]
    pub channel: Option<Channel>,
    /// Unix seconds, inclusive.
    #[serde(default)]
    pub since: Option<u64>,
    /// Unix seconds, inclusive.
    #[serde(default)]
    pub until: Option<u64>,
    /// At most this many, up to [`MAX_LIMIT`]; 100 when absent.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Most facts one query returns.
pub const MAX_LIMIT: usize = 1000;
const DEFAULT_LIMIT: usize = 100;

impl Query {
    fn matches(&self, e: &LoginEvent) -> bool {
        self.account
            .as_deref()
            .is_none_or(|a| e.account.eq_ignore_ascii_case(a.trim()))
            && self.source.is_none_or(|s| e.source_ip == s.to_canonical())
            && self.result.is_none_or(|r| e.result == r)
            && self.channel.is_none_or(|c| e.channel == c)
            && self.since.is_none_or(|s| e.occurred_at >= s)
            && self.until.is_none_or(|u| e.occurred_at <= u)
    }

    /// The number of facts to return.
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }
}

/// List and set primitives the facts need, beside [`crate::login::Ops`].
pub trait EventOps {
    /// Length of the list `key`.
    fn len(&mut self, key: &str) -> io::Result<u64>;
    /// Put `value` at the head of the list `key`, giving the list
    /// `ttl_secs` when it is new.
    fn push(&mut self, key: &str, value: &[u8], ttl_secs: u64) -> io::Result<()>;
    /// Items `start..=stop` of the list `key`, head first.
    fn range(&mut self, key: &str, start: u64, stop: u64) -> io::Result<Vec<Vec<u8>>>;
    /// Add `member` to the set `key` (which expires in `ttl_secs` when
    /// new); whether it was not there before.
    fn add_member(&mut self, key: &str, member: &str, ttl_secs: u64) -> io::Result<bool>;
    /// Add one to the counter `key`.
    fn bump(&mut self, key: &str, ttl_secs: u64) -> io::Result<()>;
}

/// `YYYYMMDD` (UTC) of the day `unix` falls in.
pub fn day(unix: u64) -> u32 {
    // days since 1970-01-01 to a civil date, after Howard Hinnant's
    // `civil_from_days`
    let z = (unix / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y * 10_000 + m * 100 + d) as u32
}

fn day_key(unix: u64) -> String {
    format!("auth:events:{}", day(unix))
}

/// Store `e`, unless the day is full (counted) or it repeats a success
/// already recorded today for the same account or key and source. Whether
/// it is a fact worth keeping, which is also whether it is worth a log
/// line.
pub fn store<O: EventOps>(ops: &mut O, e: &LoginEvent) -> io::Result<bool> {
    let retain = RETAIN_DAYS * 86_400;
    let once_a_day = matches!(e.channel, Channel::Dav | Channel::ApiKey) && e.result == Outcome::Ok;
    if once_a_day {
        let who = e.key_id.as_deref().unwrap_or(&e.account);
        let seen = format!(
            "auth:events:seen:{}:{who}:{}",
            name(&e.channel),
            day(e.occurred_at)
        );
        if !ops.add_member(&seen, &e.source_ip.to_string(), 2 * 86_400)? {
            return Ok(false);
        }
    }
    let key = day_key(e.occurred_at);
    if ops.len(&key)? >= DAY_CAP {
        ops.bump(&format!("{key}:dropped"), retain)?;
        return Ok(true);
    }
    let row = serde_json::to_vec(e).map_err(io::Error::other)?;
    ops.push(&key, &row, retain)?;
    Ok(true)
}

/// Facts matching `q`, newest first, reading back from the newest day.
pub fn query<O: EventOps>(ops: &mut O, q: &Query, now: u64) -> io::Result<Vec<LoginEvent>> {
    const PAGE: u64 = 1000;
    let limit = q.limit();
    let newest = q.until.unwrap_or(now).min(now);
    let oldest = q
        .since
        .unwrap_or(0)
        .max(now.saturating_sub(RETAIN_DAYS * 86_400));
    let mut out = Vec::new();
    let mut at = newest;
    loop {
        let key = day_key(at);
        let mut start = 0;
        loop {
            let rows = ops.range(&key, start, start + PAGE - 1)?;
            for row in &rows {
                let Ok(e) = serde_json::from_slice::<LoginEvent>(row) else {
                    continue;
                };
                if e.occurred_at < oldest {
                    return Ok(out);
                }
                if q.matches(&e) {
                    out.push(e);
                    if out.len() == limit {
                        return Ok(out);
                    }
                }
            }
            if (rows.len() as u64) < PAGE {
                break;
            }
            start += PAGE;
        }
        let Some(prev) = at.checked_sub(86_400) else {
            return Ok(out);
        };
        if day(prev) < day(oldest) {
            return Ok(out);
        }
        at = prev;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::MemoryOps;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn at(e: LoginEvent, t: u64) -> LoginEvent {
        LoginEvent {
            occurred_at: t,
            ..e
        }
    }

    #[test]
    fn days_are_civil_dates() {
        assert_eq!(day(0), 19700101);
        assert_eq!(day(951_782_400), 20000229);
        assert_eq!(day(1_792_892_646), 20261025);
    }

    #[test]
    fn the_shared_format_on_the_wire() {
        let e = LoginEvent::new(
            Channel::ApiKey,
            Credential::ApiKey,
            ip("::ffff:192.0.2.1"),
            "Hunter2",
            Outcome::BadPassword,
        )
        .with_key("mk_06fa1");
        let v = serde_json::to_value(at(e, 7)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "occurred_at": 7, "product": "mailrs", "account": "<7 chars, not an address>",
                "source_ip": "192.0.2.1", "channel": "api-key", "credential": "api-key",
                "result": "bad-password", "key_id": "mk_06fa1",
            })
        );
    }

    #[test]
    fn stored_facts_answer_queries_newest_first() {
        let mut o = MemoryOps::default();
        let t = 1_792_892_646;
        o.at(t);
        let mk = |acct: &str, src: &str, r: Outcome, when: u64| {
            at(
                LoginEvent::new(Channel::Imap, Credential::Password, ip(src), acct, r),
                when,
            )
        };
        for e in [
            mk(
                "a@x.test",
                "192.0.2.1",
                Outcome::BadPassword,
                t - 2 * 86_400,
            ),
            mk("a@x.test", "192.0.2.2", Outcome::Ok, t - 100),
            mk("b@x.test", "192.0.2.1", Outcome::Locked, t - 50),
            mk("a@x.test", "192.0.2.1", Outcome::BadPassword, t - 10),
        ] {
            store(&mut o, &e).unwrap();
        }
        let all = query(&mut o, &Query::default(), t).unwrap();
        let times: Vec<u64> = all.iter().map(|e| t - e.occurred_at).collect();
        assert_eq!(
            times,
            vec![10, 50, 100, 2 * 86_400],
            "across days, newest first"
        );
        let q = Query {
            account: Some("A@x.test".into()),
            result: Some(Outcome::BadPassword),
            ..Default::default()
        };
        assert_eq!(query(&mut o, &q, t).unwrap().len(), 2);
        let q = Query {
            source: Some(ip("192.0.2.1")),
            since: Some(t - 60),
            ..Default::default()
        };
        assert_eq!(query(&mut o, &q, t).unwrap().len(), 2);
        let q = Query {
            limit: Some(1),
            ..Default::default()
        };
        assert_eq!(query(&mut o, &q, t).unwrap()[0].occurred_at, t - 10);
    }

    #[test]
    fn per_request_successes_are_kept_once_a_day_per_source() {
        let mut o = MemoryOps::default();
        let t = 1_792_892_646;
        o.at(t);
        let ok = |src: &str| {
            LoginEvent::new(
                Channel::Dav,
                Credential::Password,
                ip(src),
                "a@x.test",
                Outcome::Ok,
            )
        };
        for _ in 0..5 {
            store(&mut o, &at(ok("192.0.2.1"), t)).unwrap();
        }
        store(&mut o, &at(ok("192.0.2.2"), t)).unwrap();
        let bad = at(
            LoginEvent::new(
                Channel::Dav,
                Credential::Password,
                ip("192.0.2.1"),
                "a@x.test",
                Outcome::BadPassword,
            ),
            t,
        );
        store(&mut o, &bad).unwrap();
        store(&mut o, &bad).unwrap();
        assert_eq!(
            query(&mut o, &Query::default(), t).unwrap().len(),
            4,
            "2 sources once, 2 failures"
        );
    }
}
