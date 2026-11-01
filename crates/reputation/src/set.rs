//! The derived set: built from every live observation, and rebuilt
//! whenever they change. Nothing here is a fact.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use crate::{Facts, Kind, Listing, Mark, Marks, Message, NeverList, Observation, Source, Stance};

const DAY: i64 = 86_400;

/// A network: an address and a prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Net {
    base: u128,
    mask: u128,
    v6: bool,
}

fn bits(ip: IpAddr) -> (u128, bool) {
    match ip {
        IpAddr::V4(a) => (u128::from(u32::from(a)), false),
        IpAddr::V6(a) => (u128::from(a), true),
    }
}

impl Net {
    pub(crate) fn parse(s: &str) -> Option<Net> {
        let (addr, len) = s.trim().split_once('/')?;
        let ip: IpAddr = addr.parse().ok()?;
        let (base, v6) = bits(ip);
        let width = match v6 {
            true => 128,
            false => 32,
        };
        let len: u32 = len.parse().ok().filter(|l| *l <= width)?;
        let mask = match len {
            0 => 0,
            l => (u128::MAX << (width - l)) & (u128::MAX >> (128 - width)),
        };
        Some(Net {
            base: base & mask,
            mask,
            v6,
        })
    }

    pub(crate) fn contains(&self, ip: IpAddr) -> bool {
        let (b, v6) = bits(ip);
        v6 == self.v6 && b & self.mask == self.base
    }
}

/// Marks kept per domain, newest first.
const MARKS_MAX: usize = 500;

#[derive(Debug, Clone)]
struct Recorded {
    obs: Observation,
}

/// Every live observation, indexed: the derived set. Built from the facts
/// and rebuilt whenever they change; nothing here is a fact.
#[derive(Debug, Clone)]
pub struct Set {
    ips: HashMap<IpAddr, Vec<Recorded>>,
    nets: Vec<(Net, Vec<Recorded>)>,
    senders: HashMap<String, Vec<Recorded>>,
    links: HashMap<String, Vec<Recorded>>,
    sender_marks: HashMap<String, Vec<Recorded>>,
    link_marks: HashMap<String, Vec<Recorded>>,
    allowed_ips: HashSet<IpAddr>,
    allowed_nets: Vec<Net>,
    allowed_hosts: HashSet<String>,
    never: NeverList,
    observations: usize,
    computed_at: i64,
}

fn normalize_host(h: &str) -> String {
    h.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// A host and every parent domain of it: `a.b.example.com`,
/// `b.example.com`, `example.com`, `com`.
pub(crate) fn host_and_parents(host: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(host);
    std::iter::from_fn(move || {
        let cur = rest?;
        rest = cur.split_once('.').map(|(_, r)| r);
        Some(cur)
    })
}

fn is_mark(s: Source) -> bool {
    matches!(s, Source::UserJunk | Source::Complaint)
}

impl Set {
    /// Build from observations at `now` (unix seconds). Retractions remove
    /// what they name; an allow removes every deny of its subject. Ages,
    /// lifetimes and thresholds are the rules' to judge.
    pub fn build(observations: &[Observation], never: &NeverList, now: i64) -> Set {
        let retracted: HashSet<&str> = observations
            .iter()
            .filter(|o| o.stance == Stance::Retract)
            .filter_map(|o| o.retracts.as_deref())
            .collect();
        let live: Vec<&Observation> = observations
            .iter()
            .filter(|o| o.stance != Stance::Retract && !retracted.contains(o.id.as_str()))
            .collect();
        let mut set = Set {
            ips: HashMap::new(),
            nets: Vec::new(),
            senders: HashMap::new(),
            links: HashMap::new(),
            sender_marks: HashMap::new(),
            link_marks: HashMap::new(),
            allowed_ips: HashSet::new(),
            allowed_nets: Vec::new(),
            allowed_hosts: HashSet::new(),
            never: never.clone(),
            observations: live.len(),
            computed_at: now,
        };
        for o in live.iter().filter(|o| o.stance == Stance::Allow) {
            match o.kind {
                Kind::Ip => set
                    .allowed_ips
                    .extend(o.value.trim().parse::<IpAddr>().ok()),
                Kind::Cidr => set.allowed_nets.extend(Net::parse(&o.value)),
                Kind::SenderDomain | Kind::LinkDomain => {
                    set.allowed_hosts.insert(normalize_host(&o.value));
                }
            }
        }
        for o in live.iter().filter(|o| o.stance == Stance::Deny) {
            let r = Recorded { obs: (*o).clone() };
            match (o.kind, is_mark(o.source)) {
                (Kind::Ip, false) => {
                    if let Ok(ip) = o.value.trim().parse() {
                        set.ips.entry(ip).or_default().push(r);
                    }
                }
                (Kind::Cidr, false) => {
                    if let Some(n) = Net::parse(&o.value) {
                        set.nets.push((n, vec![r]));
                    }
                }
                (Kind::SenderDomain, false) => set
                    .senders
                    .entry(normalize_host(&o.value))
                    .or_default()
                    .push(r),
                (Kind::LinkDomain, false) => set
                    .links
                    .entry(normalize_host(&o.value))
                    .or_default()
                    .push(r),
                (Kind::SenderDomain, true) => set
                    .sender_marks
                    .entry(crate::registrable(&o.value))
                    .or_default()
                    .push(r),
                (Kind::LinkDomain, true) => set
                    .link_marks
                    .entry(crate::registrable(&o.value))
                    .or_default()
                    .push(r),
                _ => {}
            }
        }
        for marks in set
            .sender_marks
            .values_mut()
            .chain(set.link_marks.values_mut())
        {
            marks.sort_by_key(|r| std::cmp::Reverse((r.obs.at, r.obs.id.clone())));
            marks.truncate(MARKS_MAX);
        }
        set
    }

    /// Live observations it was built from, for health.
    pub fn observations(&self) -> usize {
        self.observations
    }

    /// When it was built, for health.
    pub fn computed_at(&self) -> i64 {
        self.computed_at
    }

    fn host_allowed(&self, host: &str) -> bool {
        host_and_parents(host).any(|h| self.allowed_hosts.contains(h))
    }

    fn ip_allowed(&self, ip: IpAddr) -> bool {
        self.allowed_ips.contains(&ip) || self.allowed_nets.iter().any(|n| n.contains(ip))
    }

    fn listing(&self, r: &Recorded) -> Listing {
        Listing {
            subject: r.obs.value.trim().to_ascii_lowercase(),
            id: r.obs.id.clone(),
            source: r.obs.source,
            age_days: (self.computed_at - r.obs.at).max(0) / DAY,
            reason: r.obs.reason.clone(),
        }
    }

    fn hosts(&self, map: &HashMap<String, Vec<Recorded>>, host: &str) -> Vec<Listing> {
        let host = normalize_host(host);
        if host.is_empty() || self.host_allowed(&host) {
            return Vec::new();
        }
        host_and_parents(&host)
            .filter_map(|h| map.get(h))
            .flatten()
            .map(|r| self.listing(r))
            .collect()
    }

    fn marks(&self, map: &HashMap<String, Vec<Recorded>>, host: &str) -> Option<Marks> {
        let host = normalize_host(host);
        if host.is_empty() || self.host_allowed(&host) {
            return None;
        }
        let domain = crate::registrable(&host);
        let recorded = map.get(&domain)?;
        Some(Marks {
            never: self.never.contains(&domain),
            marks: recorded
                .iter()
                .map(|r| Mark {
                    id: r.obs.id.clone(),
                    source: r.obs.source,
                    actor: r.obs.actor.clone().unwrap_or_default(),
                    age_days: (self.computed_at - r.obs.at).max(0) / DAY,
                })
                .collect(),
            domain,
        })
    }

    /// The listings for a host a link points at; empty when it is not
    /// listed or is allowed.
    pub fn link_listings(&self, host: &str) -> Vec<Listing> {
        self.hosts(&self.links, host)
    }

    /// What the set knows about one message. An allowed subject has no
    /// listings and no marks.
    pub fn facts(&self, m: &Message) -> Facts {
        let ip = m
            .client_ip
            .filter(|ip| !self.ip_allowed(*ip))
            .map(|ip| {
                let exact = self.ips.get(&ip).into_iter().flatten();
                let nets = self
                    .nets
                    .iter()
                    .filter(|(n, _)| n.contains(ip))
                    .flat_map(|(_, r)| r);
                exact.chain(nets).map(|r| self.listing(r)).collect()
            })
            .unwrap_or_default();
        let mut link_marks: Vec<Marks> = Vec::new();
        let mut links = Vec::new();
        for h in &m.link_hosts {
            links.extend(self.hosts(&self.links, h));
            if let Some(mk) = self.marks(&self.link_marks, h)
                && !link_marks.iter().any(|x| x.domain == mk.domain)
            {
                link_marks.push(mk);
            }
        }
        Facts {
            ip,
            envelope: self.hosts(&self.senders, &m.envelope_domain),
            header: self.hosts(&self.senders, &m.header_domain),
            links,
            sender_marks: self.marks(&self.sender_marks, &m.header_domain),
            link_marks,
            dmarc_fail: m.auth.dmarc_fail,
            spf_fail: m.auth.spf_fail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn networks() {
        let n = Net::parse("203.0.113.0/24").unwrap();
        assert!(n.contains("203.0.113.9".parse().unwrap()));
        assert!(!n.contains("203.0.114.1".parse().unwrap()));
        assert!(!n.contains("::1".parse().unwrap()));
        let v6 = Net::parse("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8:1::5".parse().unwrap()));
        assert!(
            Net::parse("0.0.0.0/0")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        for bad in ["203.0.113.0", "203.0.113.0/33", "x/8", "::/129"] {
            assert!(Net::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn parents() {
        let all: Vec<&str> = host_and_parents("a.b.example.com").collect();
        assert_eq!(
            all,
            ["a.b.example.com", "b.example.com", "example.com", "com"]
        );
    }
}
