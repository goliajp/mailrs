//! The security alert mail: one message for whatever the login guard,
//! the outbound pause, the backup and the complaint loop queued since the
//! last one, written for the person who has to decide what to do.
//!
//! Each alert says what happened in a sentence, how urgent it is, what it
//! means and what to do, then the facts behind it. Times are dates in the
//! operator's time zone with how long ago or how far off, never unix
//! seconds. The mail is HTML with a plain-text twin. The language and the
//! time zone are `MAILRS_SECURITY_ALERT_LANG` (`zh` or `en`, English when
//! unset) and `MAILRS_SECURITY_ALERT_UTC_OFFSET` (such as `+09:00`, UTC
//! when unset).

mod copy;
mod layout;

use mailrs_auth_guard::alerts::{Alert, Fact};

pub use copy::Level;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// The operator's clock: an offset from UTC and how to name it.
#[derive(Debug, Clone)]
pub struct Clock {
    pub offset_secs: i64,
    pub name: String,
}

/// Where the mail is read: its language and time zone.
#[derive(Debug, Clone)]
pub struct Reader {
    pub lang: Lang,
    pub clock: Clock,
}

impl Reader {
    /// From `MAILRS_SECURITY_ALERT_LANG` and `MAILRS_SECURITY_ALERT_UTC_OFFSET`.
    /// A value it cannot read is logged and the default used.
    pub fn from_env() -> Self {
        let lang = match std::env::var("MAILRS_SECURITY_ALERT_LANG").as_deref() {
            Ok("zh") => Lang::Zh,
            Ok("en") | Err(_) => Lang::En,
            Ok(other) => {
                tracing::error!(
                    value = other,
                    "MAILRS_SECURITY_ALERT_LANG: zh or en; using en"
                );
                Lang::En
            }
        };
        let offset = std::env::var("MAILRS_SECURITY_ALERT_UTC_OFFSET").unwrap_or_default();
        let clock = match parse_offset(&offset) {
            Some(c) => c,
            None => {
                if !offset.is_empty() {
                    tracing::error!(
                        value = %offset,
                        "MAILRS_SECURITY_ALERT_UTC_OFFSET: like +09:00; using UTC"
                    );
                }
                Clock {
                    offset_secs: 0,
                    name: "UTC".into(),
                }
            }
        };
        Self { lang, clock }
    }
}

/// `+09:00`, `-05:30` or `+9` as a clock; `None` for anything else.
pub fn parse_offset(s: &str) -> Option<Clock> {
    let s = s.trim();
    let (sign, rest) = match s.chars().next()? {
        '+' => (1, &s[1..]),
        '-' => (-1, &s[1..]),
        _ => return None,
    };
    let (h, m) = match rest.split_once(':') {
        Some((h, m)) => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?),
        None => (rest.parse::<i64>().ok()?, 0),
    };
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    let name = match (m, sign) {
        (0, 1) => format!("UTC+{h}"),
        (0, _) => format!("UTC-{h}"),
        (_, 1) => format!("UTC+{h}:{m:02}"),
        (_, _) => format!("UTC-{h}:{m:02}"),
    };
    Some(Clock {
        offset_secs: sign * (h * 3600 + m * 60),
        name,
    })
}

/// The mail for `alerts`.
pub struct Mail {
    pub subject: String,
    pub text: String,
    pub html: String,
}

/// One alert as the mail shows it.
pub struct Shown {
    pub copy: copy::Copy,
    pub when: String,
    pub facts: Vec<(String, String)>,
}

/// Write the mail for `alerts` on `host`, read by `reader` at `now`.
pub fn render(alerts: &[Alert], host: &str, reader: &Reader, now: u64) -> Mail {
    let mut shown: Vec<Shown> = alerts
        .iter()
        .map(|a| Shown {
            copy: copy::copy(a, reader.lang),
            when: when(a.at, reader, now),
            facts: facts(a, reader, now),
        })
        .collect();
    shown.sort_by_key(|s| s.copy.level);
    let subject = subject(&shown, host, reader.lang);
    let text = layout::text(&shown, host, reader);
    let html = layout::html(&shown, host, reader);
    Mail {
        subject,
        text,
        html,
    }
}

fn subject(shown: &[Shown], host: &str, lang: Lang) -> String {
    let act = shown.iter().filter(|s| s.copy.level == Level::Act).count();
    let head = match (shown, lang) {
        ([one], _) => one.copy.title.clone(),
        (many, Lang::Zh) if act > 0 => format!("{} 件事，其中 {act} 件需要处理", many.len()),
        (many, Lang::Zh) => format!("{} 件事", many.len()),
        (many, Lang::En) if act > 0 => format!("{} alerts, {act} need action", many.len()),
        (many, Lang::En) => format!("{} alerts", many.len()),
    };
    match lang {
        Lang::Zh => format!("[{host}] 安全提醒：{head}"),
        Lang::En => format!("[{host}] Security: {head}"),
    }
}

/// `2026-10-31 15:19 (UTC+9), 3 hours ago`.
pub fn when(at: u64, reader: &Reader, now: u64) -> String {
    let date = date(at, &reader.clock);
    let rel = relative(at as i64 - now as i64, reader.lang);
    match reader.lang {
        Lang::Zh => format!("{date}（{}），{rel}", reader.clock.name),
        Lang::En => format!("{date} ({}), {rel}", reader.clock.name),
    }
}

fn date(at: u64, clock: &Clock) -> String {
    let local = at as i64 + clock.offset_secs;
    match chrono::DateTime::from_timestamp(local, 0) {
        Some(d) => d.format("%Y-%m-%d %H:%M").to_string(),
        None => at.to_string(),
    }
}

/// How far `delta` seconds is from now, in words.
pub fn relative(delta: i64, lang: Lang) -> String {
    let span = duration(delta.unsigned_abs(), lang);
    match (delta.signum(), lang) {
        (0, Lang::Zh) => "刚刚".into(),
        (0, Lang::En) => "just now".into(),
        (1, Lang::Zh) => format!("{span}后"),
        (1, Lang::En) => format!("in {span}"),
        (_, Lang::Zh) => format!("{span}前"),
        (_, Lang::En) => format!("{span} ago"),
    }
}

/// A length of time in the largest unit that fits.
pub fn duration(secs: u64, lang: Lang) -> String {
    let (n, unit) = match secs {
        s if s < 60 => (s, 0),
        s if s < 3600 => (s / 60, 1),
        s if s < 2 * 86_400 => (s / 3600, 2),
        s => (s / 86_400, 3),
    };
    let plural = |w: &str| match n {
        1 => format!("1 {w}"),
        _ => format!("{n} {w}s"),
    };
    match (lang, unit) {
        (Lang::Zh, 0) => format!("{n} 秒"),
        (Lang::Zh, 1) => format!("{n} 分钟"),
        (Lang::Zh, 2) => format!("约 {n} 小时"),
        (Lang::Zh, _) => format!("{n} 天"),
        (Lang::En, 0) => plural("second"),
        (Lang::En, 1) => plural("minute"),
        (Lang::En, 2) => format!("about {}", plural("hour")),
        (Lang::En, _) => plural("day"),
    }
}

fn facts(a: &Alert, reader: &Reader, now: u64) -> Vec<(String, String)> {
    let lang = reader.lang;
    let zh = lang == Lang::Zh;
    let pick = |z: &str, e: &str| match zh {
        true => z.to_string(),
        false => e.to_string(),
    };
    let mins = |m: u64| duration(m * 60, lang);
    let mut out: Vec<(String, String)> = a
        .facts
        .iter()
        .map(|f| match f {
            Fact::Failures { count, minutes } => (
                pick("登录失败", "Failed sign-ins"),
                match zh {
                    true => format!("{count} 次（{}内）", mins(*minutes)),
                    false => format!("{count} within {}", mins(*minutes)),
                },
            ),
            Fact::LastAttempt { source, account } => (
                pick("最后一次尝试", "Last attempt"),
                match zh {
                    true => format!("来自 {source}，登录名 {account}"),
                    false => format!("from {source}, as {account}"),
                },
            ),
            Fact::LockedFor { minutes } => (pick("锁定时长", "Locked for"), mins(*minutes)),
            Fact::Networks { list, minutes } => (
                pick("登录的网络", "Networks"),
                match zh {
                    true => format!("{}（{}内）", list.join("、"), mins(*minutes)),
                    false => format!("{} (within {})", list.join(", "), mins(*minutes)),
                },
            ),
            Fact::IssuedBy { actor } => (pick("签发人", "Issued by"), actor.clone()),
            Fact::ValidUntil { at } => (pick("有效期至", "Valid until"), when(*at, reader, now)),
            Fact::Session {
                prefix,
                source,
                failures,
            } => (
                pick("这次登录", "The sign-in"),
                match zh {
                    true => format!("编号 {prefix}…，来自 {source}，密码确认输错 {failures} 次"),
                    false => format!(
                        "id {prefix}…, from {source}, {failures} failed password confirmations"
                    ),
                },
            ),
            Fact::SendLimit { count, limit } => (
                pick("一小时内发给", "Sent in an hour"),
                match zh {
                    true => format!("{count} 位收件人（上限 {limit}）"),
                    false => format!("{count} recipients (limit {limit})"),
                },
            ),
            Fact::FailureLimit { count, limit } => (
                pick("一小时内退信", "Bounced in an hour"),
                match zh {
                    true => format!("{count} 封（上限 {limit}）"),
                    false => format!("{count} messages (limit {limit})"),
                },
            ),
            Fact::PausedSince { at } => {
                (pick("暂停开始于", "Paused since"), when(*at, reader, now))
            }
            Fact::Discarded { count } => (
                pick("已丢弃", "Discarded"),
                match zh {
                    true => format!("{count} 封扣下太久的邮件，已给发件人退信"),
                    false => format!(
                        "{count} held messages past the queue's lifetime; their senders were told"
                    ),
                },
            ),
            Fact::NoBackupFor { hours } => (
                pick("上次成功备份", "Last good backup"),
                relative(-((*hours * 3600) as i64), lang),
            ),
            Fact::Complaints { reporter, limit } => (
                pick("投诉来自", "Complaints from"),
                match zh {
                    true => format!("{reporter}，每天处理上限 {limit} 条"),
                    false => format!("{reporter}, {limit} a day are acted on"),
                },
            ),
            Fact::LatestComplaint { address } => (
                pick("最近一条投诉的人", "Latest complaint from"),
                address.clone(),
            ),
            Fact::Note { text } => (pick("系统记录", "System note"), text.clone()),
        })
        .collect();
    // queued before facts were recorded: the English line is all there is
    if out.is_empty() && !a.detail.is_empty() {
        out.push((pick("详情", "Details"), a.detail.clone()));
    }
    out
}

#[cfg(test)]
mod tests;
