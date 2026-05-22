#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::HashMap;
use std::fmt::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hickory_resolver::TokioResolver;

/// Reverse an IPv4 address into the dotted-octets form used for DNSBL
/// lookup per RFC 5782 §2.1: `1.2.3.4` → `"4.3.2.1"`.
pub fn reverse_ipv4(ip: Ipv4Addr) -> String {
    let o = ip.octets();
    // Max length: 4 octets × 3 chars + 3 dots = 15 chars.
    let mut out = String::with_capacity(15);
    write!(&mut out, "{}.{}.{}.{}", o[3], o[2], o[1], o[0]).unwrap();
    out
}

/// Build a DNSBL query hostname: `<reversed_ip>.<zone>` per RFC 5782 §2.1.
pub fn dnsbl_query(reversed: &str, zone: &str) -> String {
    let mut out = String::with_capacity(reversed.len() + 1 + zone.len());
    out.push_str(reversed);
    out.push('.');
    out.push_str(zone);
    out
}

/// Spamhaus return codes (127.0.0.x) per <https://www.spamhaus.org/zen/>.
///
/// `Clean` covers both "not listed" (NXDOMAIN) and "listed under
/// 127.0.0.0" (test record). Real listings populate the `Sbl` / `Css`
/// / `Xbl` / `Pbl` variants; non-Spamhaus DNSBLs that return other
/// 127.0.0.x codes hit `Listed(other)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsblResult {
    /// Not listed (NXDOMAIN or 127.0.0.0).
    Clean,
    /// Listed in SBL (Spamhaus Block List).
    Sbl,
    /// Listed in CSS (Combined Spam Sources).
    Css,
    /// Listed in XBL (Exploits Block List).
    Xbl,
    /// Listed in PBL (Policy Block List).
    Pbl,
    /// Listed but the return code is outside the documented Spamhaus
    /// range. Other DNSBLs (e.g. Barracuda) may use custom codes.
    Listed(u8),
}

/// Interpret a Spamhaus-style A record response as a [`DnsblResult`].
///
/// Anything outside 127.0.0.0/24 (or the 0 / 1 sentinels) is treated
/// as `Clean`. The codes follow Spamhaus's published ranges; other
/// DNSBL operators that share the 127.0.0.x convention but with
/// different code-to-list mapping will hit `DnsblResult::Listed(code)`.
pub fn interpret_spamhaus(ip: Ipv4Addr) -> DnsblResult {
    let octets = ip.octets();
    if octets[0] != 127 || octets[1] != 0 || octets[2] != 0 {
        return DnsblResult::Clean;
    }
    match octets[3] {
        2 => DnsblResult::Sbl,
        3 => DnsblResult::Css,
        4..=7 => DnsblResult::Xbl,
        10 | 11 => DnsblResult::Pbl,
        0 => DnsblResult::Clean,
        other => DnsblResult::Listed(other),
    }
}

/// Stub: IPv6 isn't supported by most DNSBL operators (the reverse-IP
/// scheme is impractical at IPv6 scale). Always returns `false`. Kept
/// as an extension point in case a future operator adopts IPv6 lookups.
pub fn is_ipv6_dnsbl_supported(_ip: &Ipv6Addr) -> bool {
    false
}

/// Run the DNSBL lookup against each zone in order; return the first
/// zone that lists `ip`, or `None` if no zone lists it.
///
/// IPv6 addresses are short-circuited (see [`is_ipv6_dnsbl_supported`]).
/// DNS lookup failures (NXDOMAIN, SERVFAIL, timeout) are treated as
/// "not listed" — the function returns `None`, not an error.
pub async fn check_dnsbl(
    resolver: &TokioResolver,
    ip: IpAddr,
    zones: &[String],
) -> Option<(String, DnsblResult)> {
    let ipv4 = match ip {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(v6) => {
            if !is_ipv6_dnsbl_supported(&v6) {
                return None;
            }
            return None;
        }
    };

    let reversed = reverse_ipv4(ipv4);

    for zone in zones {
        let query_host = dnsbl_query(&reversed, zone);
        if let Ok(response) = resolver.ipv4_lookup(&query_host).await {
            for record in response.answers() {
                if let hickory_resolver::proto::rr::RData::A(addr) = &record.data {
                    let result = interpret_spamhaus(addr.0);
                    if result != DnsblResult::Clean {
                        return Some((zone.clone(), result));
                    }
                }
            }
        }
    }

    None
}

/// TTL-cached DNSBL lookup. Avoids repeated DNS queries for IPs we've
/// seen recently.
///
/// Caches **both** positive results (listed → known bad) and negative
/// results (not listed → known good). The TTL applies uniformly; if
/// you want different positive vs negative TTLs, run two caches.
///
/// Storage is `Mutex<HashMap>` — fine for sub-1k entries with moderate
/// throughput. For higher contention, wrap your own `DashMap` and call
/// [`check_dnsbl`] directly.
pub struct DnsblCache {
    #[allow(clippy::type_complexity)]
    cache: Mutex<HashMap<IpAddr, (Option<(String, DnsblResult)>, Instant)>>,
    ttl: Duration,
}

impl DnsblCache {
    /// Construct an empty cache with the given per-entry TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// check with cache: return cached result if fresh, otherwise query DNS
    pub async fn check(
        &self,
        resolver: &TokioResolver,
        ip: IpAddr,
        zones: &[String],
    ) -> Option<(String, DnsblResult)> {
        // check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some((result, inserted_at)) = cache.get(&ip)
                && inserted_at.elapsed() < self.ttl {
                    return result.clone();
                }
        }

        // cache miss or expired — query
        let result = check_dnsbl(resolver, ip, zones).await;

        // store in cache (including None for negative caching)
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(ip, (result.clone(), Instant::now()));
        }

        result
    }

    /// remove expired entries
    pub fn cleanup(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.retain(|_, (_, inserted_at)| inserted_at.elapsed() < self.ttl);
    }

    /// number of cached entries (for testing)
    pub fn len(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    /// check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_ipv4_standard() {
        assert_eq!(reverse_ipv4(Ipv4Addr::new(1, 2, 3, 4)), "4.3.2.1");
    }

    #[test]
    fn reverse_ipv4_loopback() {
        assert_eq!(reverse_ipv4(Ipv4Addr::new(127, 0, 0, 1)), "1.0.0.127");
    }

    #[test]
    fn dnsbl_query_format() {
        let reversed = reverse_ipv4(Ipv4Addr::new(10, 20, 30, 40));
        let query = dnsbl_query(&reversed, "zen.spamhaus.org");
        assert_eq!(query, "40.30.20.10.zen.spamhaus.org");
    }

    #[test]
    fn interpret_spamhaus_sbl() {
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 2)),
            DnsblResult::Sbl
        );
    }

    #[test]
    fn interpret_spamhaus_xbl() {
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 4)),
            DnsblResult::Xbl
        );
    }

    #[test]
    fn interpret_spamhaus_pbl() {
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 10)),
            DnsblResult::Pbl
        );
    }

    #[test]
    fn interpret_spamhaus_clean() {
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 0)),
            DnsblResult::Clean
        );
        // non-127.0.0.x should also be clean
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(192, 168, 1, 1)),
            DnsblResult::Clean
        );
    }

    #[test]
    fn ipv6_not_supported() {
        assert!(!is_ipv6_dnsbl_supported(&Ipv6Addr::LOCALHOST));
    }

    #[test]
    fn dnsbl_cache_negative() {
        let cache = DnsblCache::new(Duration::from_secs(300));

        // insert a negative (clean) result
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(ip, (None, Instant::now()));
        }

        // cache should have the entry
        assert_eq!(cache.len(), 1);

        // verify it's a negative entry
        let c = cache.cache.lock().unwrap();
        let (result, _) = c.get(&ip).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn dnsbl_cache_cleanup_expired() {
        let cache = DnsblCache::new(Duration::from_millis(1));

        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(
                ip,
                (
                    Some(("zen.spamhaus.org".into(), DnsblResult::Sbl)),
                    Instant::now() - Duration::from_secs(10),
                ),
            );
        }

        cache.cleanup();
        assert!(cache.is_empty());
    }

    // ===== additional edge cases =====

    #[test]
    fn reverse_ipv4_zero_address() {
        assert_eq!(reverse_ipv4(Ipv4Addr::UNSPECIFIED), "0.0.0.0");
    }

    #[test]
    fn reverse_ipv4_broadcast() {
        assert_eq!(reverse_ipv4(Ipv4Addr::BROADCAST), "255.255.255.255");
    }

    #[test]
    fn dnsbl_query_handles_trailing_dot_in_zone() {
        // RFC-strict zones might be supplied with a trailing dot; we
        // don't strip it. Documented behavior.
        let r = reverse_ipv4(Ipv4Addr::new(1, 2, 3, 4));
        let q = dnsbl_query(&r, "spamhaus.org.");
        assert_eq!(q, "4.3.2.1.spamhaus.org.");
    }

    #[test]
    fn interpret_spamhaus_css_code() {
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 3)),
            DnsblResult::Css
        );
    }

    #[test]
    fn interpret_spamhaus_xbl_range_all_codes() {
        for code in 4..=7u8 {
            assert_eq!(
                interpret_spamhaus(Ipv4Addr::new(127, 0, 0, code)),
                DnsblResult::Xbl,
                "code {code} should be XBL"
            );
        }
    }

    #[test]
    fn interpret_spamhaus_pbl_both_codes() {
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 10)),
            DnsblResult::Pbl
        );
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 11)),
            DnsblResult::Pbl
        );
    }

    #[test]
    fn interpret_spamhaus_unknown_code_falls_through() {
        // Code 99 isn't documented — should yield Listed(99) not Clean.
        let r = interpret_spamhaus(Ipv4Addr::new(127, 0, 0, 99));
        assert_eq!(r, DnsblResult::Listed(99));
    }

    #[test]
    fn interpret_spamhaus_almost_127_but_not_quite() {
        // 127.0.1.x is OUTSIDE the documented Spamhaus range — Clean.
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 0, 1, 2)),
            DnsblResult::Clean
        );
        assert_eq!(
            interpret_spamhaus(Ipv4Addr::new(127, 1, 0, 2)),
            DnsblResult::Clean
        );
    }

    #[test]
    fn dnsbl_cache_double_lookup_returns_same() {
        let cache = DnsblCache::new(Duration::from_secs(300));
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        // pre-seed a positive entry
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(
                ip,
                (
                    Some(("zen.spamhaus.org".into(), DnsblResult::Sbl)),
                    Instant::now(),
                ),
            );
        }
        // Read it twice — both reads see the same value.
        let c = cache.cache.lock().unwrap();
        let (r1, _) = c.get(&ip).unwrap();
        let (r2, _) = c.get(&ip).unwrap();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
    }

    #[test]
    fn dnsbl_cache_is_empty_on_fresh() {
        let cache = DnsblCache::new(Duration::from_secs(60));
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn dnsbl_cache_cleanup_preserves_fresh() {
        let cache = DnsblCache::new(Duration::from_secs(300));
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 99));
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(ip, (None, Instant::now())); // negative, fresh
        }
        cache.cleanup();
        assert_eq!(cache.len(), 1); // still there
    }

    #[test]
    fn is_ipv6_dnsbl_supported_always_false() {
        assert!(!is_ipv6_dnsbl_supported(&Ipv6Addr::LOCALHOST));
        assert!(!is_ipv6_dnsbl_supported(&Ipv6Addr::UNSPECIFIED));
        // Even a Spamhaus-style v6 still rejected — the function is
        // a documented stub.
        assert!(!is_ipv6_dnsbl_supported(
            &"2001:db8::1".parse::<Ipv6Addr>().unwrap()
        ));
    }

    #[test]
    fn dnsbl_query_with_empty_zone() {
        // Edge: empty zone. Documented behavior: returns "reversed."
        let q = dnsbl_query("4.3.2.1", "");
        assert_eq!(q, "4.3.2.1.");
    }
}
