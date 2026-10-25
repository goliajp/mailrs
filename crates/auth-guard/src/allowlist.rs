//! Source allowlists for listeners: off unless configured. A deployment
//! that wants a port open only to some networks sets
//! `MAILRS_<LISTENER>_ALLOW` to comma-separated CIDRs; a connection from
//! anywhere else is closed before anything is said to it.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use ipnet::IpNet;

/// Who a listener accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist(Vec<IpNet>);

impl Allowlist {
    /// The list `var` names: `Ok(None)` when it is not set (everyone is
    /// let in). Set but empty, or holding something that is not a CIDR, is
    /// an error: whether an empty list meant "everyone" or "nobody" is not
    /// something to guess on a mail server.
    pub fn from_env(var: &str) -> Result<Option<Self>, String> {
        match std::env::var(var) {
            Err(_) => Ok(None),
            Ok(raw) => Self::parse(&raw)
                .map(Some)
                .map_err(|e| format!("{var}: {e}")),
        }
    }

    /// `raw` as a list; see [`Allowlist::from_env`].
    pub fn parse(raw: &str) -> Result<Self, String> {
        let nets = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<IpNet>()
                    .or_else(|_| s.parse::<IpAddr>().map(IpNet::from))
                    .map_err(|_| format!("{s:?} is not an address or CIDR"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if nets.is_empty() {
            return Err(
                "empty; leave it unset to accept every source, or name the networks".into(),
            );
        }
        Ok(Self(nets))
    }

    /// Whether a connection from `ip` is accepted.
    pub fn allows(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.0.iter().any(|n| n.contains(&ip))
    }
}

/// Refused connections, remembered so each source is recorded once per
/// [`DenialLog::WINDOW_SECS`] instead of once per connection.
#[derive(Debug, Default)]
pub struct DenialLog {
    last: Mutex<HashMap<IpAddr, u64>>,
}

impl DenialLog {
    /// How long one record stands for a source's refused connections.
    pub const WINDOW_SECS: u64 = 600;

    /// Whether this refusal is the first from `ip` in the window.
    pub fn first(&self, ip: IpAddr, now: u64) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.len() > 10_000 {
            last.retain(|_, at| now.saturating_sub(*at) < Self::WINDOW_SECS);
        }
        match last.get(&ip) {
            Some(at) if now.saturating_sub(*at) < Self::WINDOW_SECS => false,
            _ => {
                last.insert(ip, now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_list_names_networks_and_single_addresses() {
        let a = Allowlist::parse("10.0.0.0/8, 203.0.113.7 ,2001:db8::/32").unwrap();
        assert!(a.allows(ip("10.1.2.3")));
        assert!(a.allows(ip("203.0.113.7")));
        assert!(
            a.allows(ip("::ffff:10.9.9.9")),
            "IPv4-mapped is the IPv4 address"
        );
        assert!(a.allows(ip("2001:db8:1::1")));
        assert!(!a.allows(ip("203.0.113.8")));
        assert!(!a.allows(ip("192.0.2.1")));
    }

    #[test]
    fn empty_or_malformed_lists_are_refused() {
        assert!(Allowlist::parse("").is_err());
        assert!(Allowlist::parse(" , ").is_err());
        assert!(Allowlist::parse("10.0.0.0/8,office").is_err());
    }

    #[test]
    fn unset_means_no_list() {
        assert_eq!(
            Allowlist::from_env("MAILRS_TEST_SURELY_UNSET_ALLOW"),
            Ok(None)
        );
    }

    #[test]
    fn a_source_is_recorded_once_a_window() {
        let log = DenialLog::default();
        assert!(log.first(ip("192.0.2.1"), 1000));
        assert!(!log.first(ip("192.0.2.1"), 1100));
        assert!(log.first(ip("192.0.2.2"), 1100));
        assert!(log.first(ip("192.0.2.1"), 1000 + DenialLog::WINDOW_SECS));
    }
}
