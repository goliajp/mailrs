//! Who signs in to the mail protocols with what: the account password or
//! an app password, per account and protocol, from the stored sign-in
//! facts. What the switch to app passwords is decided from.

use std::collections::BTreeMap;
use std::io;

use crate::events::{Channel, Credential, EventOps, LoginEvent, Outcome};

/// One account's successful sign-ins on one protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Usage {
    /// The account, lowercased.
    pub account: String,
    /// The protocol.
    pub channel: String,
    /// Sign-ins with the account password (DAV counts once a day per
    /// source).
    pub password_logins: u64,
    /// Unix seconds of the latest one.
    pub password_last: Option<u64>,
    /// Sign-ins with an app password.
    pub app_password_logins: u64,
    /// Unix seconds of the latest one.
    pub app_password_last: Option<u64>,
}

fn protocol(c: Channel) -> Option<&'static str> {
    match c {
        Channel::Imap => Some("imap"),
        Channel::Pop3 => Some("pop3"),
        Channel::Smtp => Some("smtp"),
        Channel::Managesieve => Some("managesieve"),
        Channel::Dav => Some("dav"),
        Channel::Jmap => Some("jmap"),
        _ => None,
    }
}

/// Successful mail-protocol sign-ins in the last `days` days, by account
/// and protocol, accounts in order.
///
/// # Errors
/// The store could not be read.
pub fn usage<O: EventOps>(ops: &mut O, days: u64, now: u64) -> io::Result<Vec<Usage>> {
    const PAGE: u64 = 1000;
    let oldest = now.saturating_sub(days.min(crate::events::RETAIN_DAYS) * 86_400);
    let mut out: BTreeMap<(String, &'static str), Usage> = BTreeMap::new();
    let mut at = now;
    while at >= oldest {
        let key = crate::events::day_key(at);
        let mut start = 0;
        loop {
            let rows = ops.range(&key, start, start + PAGE - 1)?;
            for row in &rows {
                let Ok(e) = serde_json::from_slice::<LoginEvent>(row) else {
                    continue;
                };
                let Some(ch) = protocol(e.channel) else {
                    continue;
                };
                if e.result != Outcome::Ok || e.occurred_at < oldest || e.account.is_empty() {
                    continue;
                }
                let account = e.account.to_ascii_lowercase();
                let u = out.entry((account.clone(), ch)).or_insert_with(|| Usage {
                    account,
                    channel: ch.to_string(),
                    ..Usage::default()
                });
                let (n, last) = match e.credential {
                    Credential::AppPassword => {
                        (&mut u.app_password_logins, &mut u.app_password_last)
                    }
                    _ => (&mut u.password_logins, &mut u.password_last),
                };
                *n += 1;
                *last = Some(last.map_or(e.occurred_at, |l| l.max(e.occurred_at)));
            }
            if (rows.len() as u64) < PAGE {
                break;
            }
            start += PAGE;
        }
        let Some(prev) = at.checked_sub(86_400) else {
            break;
        };
        at = prev;
    }
    Ok(out.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::MemoryOps;

    fn ev(
        channel: Channel,
        credential: Credential,
        account: &str,
        result: Outcome,
        at: u64,
    ) -> LoginEvent {
        LoginEvent {
            occurred_at: at,
            ..LoginEvent::new(
                channel,
                credential,
                "192.0.2.1".parse().unwrap(),
                account,
                result,
            )
        }
    }

    #[test]
    fn counts_successful_protocol_sign_ins_by_credential() {
        let now = 1_800_000_000;
        let mut ops = MemoryOps::default();
        for e in [
            ev(
                Channel::Imap,
                Credential::Password,
                "A@x.jp",
                Outcome::Ok,
                now - 10,
            ),
            ev(
                Channel::Imap,
                Credential::Password,
                "a@x.jp",
                Outcome::Ok,
                now - 86_400,
            ),
            ev(
                Channel::Imap,
                Credential::AppPassword,
                "a@x.jp",
                Outcome::Ok,
                now - 5,
            ),
            ev(
                Channel::Smtp,
                Credential::Password,
                "a@x.jp",
                Outcome::BadPassword,
                now - 5,
            ),
            ev(
                Channel::Web,
                Credential::Password,
                "a@x.jp",
                Outcome::Ok,
                now - 5,
            ),
            ev(
                Channel::Pop3,
                Credential::Password,
                "b@x.jp",
                Outcome::Ok,
                now - 40 * 86_400,
            ),
        ] {
            crate::events::store(&mut ops, &e).unwrap();
        }
        let got = usage(&mut ops, 30, now).unwrap();
        assert_eq!(
            got,
            vec![Usage {
                account: "a@x.jp".into(),
                channel: "imap".into(),
                password_logins: 2,
                password_last: Some(now - 10),
                app_password_logins: 1,
                app_password_last: Some(now - 5),
            }]
        );
        assert_eq!(
            usage(&mut ops, 60, now).unwrap().len(),
            2,
            "b's POP3 is 40 days old"
        );
    }
}
