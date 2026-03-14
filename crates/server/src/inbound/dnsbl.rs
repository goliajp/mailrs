use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hickory_resolver::TokioResolver;

/// reverse an IPv4 address for DNSBL lookup: 1.2.3.4 → "4.3.2.1"
pub fn reverse_ipv4(ip: Ipv4Addr) -> String {
    let o = ip.octets();
    format!("{}.{}.{}.{}", o[3], o[2], o[1], o[0])
}

/// build DNSBL query hostname: reversed_ip.zone
pub fn dnsbl_query(reversed: &str, zone: &str) -> String {
    format!("{reversed}.{zone}")
}

/// spamhaus return codes (127.0.0.x)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsblResult {
    /// not listed
    Clean,
    /// listed in SBL (spamhaus block list)
    Sbl,
    /// listed in CSS (combined spam sources)
    Css,
    /// listed in XBL (exploits block list)
    Xbl,
    /// listed in PBL (policy block list)
    Pbl,
    /// listed but unknown code
    Listed(u8),
}

/// interpret a spamhaus-style A record response
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

/// IPv6 is not supported for DNSBL (most lists don't support it)
pub fn is_ipv6_dnsbl_supported(_ip: &Ipv6Addr) -> bool {
    false
}

/// perform actual DNS query against DNSBL zones
/// returns the first zone that lists the IP with its result
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
            for addr in response.iter() {
                let result = interpret_spamhaus(addr.0);
                if result != DnsblResult::Clean {
                    return Some((zone.clone(), result));
                }
            }
        }
    }

    None
}

/// cached DNSBL lookup to avoid repeated queries for known IPs
/// caches both positive (listed) and negative (clean) results
pub struct DnsblCache {
    #[allow(clippy::type_complexity)]
    cache: Mutex<HashMap<IpAddr, (Option<(String, DnsblResult)>, Instant)>>,
    ttl: Duration,
}

impl DnsblCache {
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
            if let Some((result, inserted_at)) = cache.get(&ip) {
                if inserted_at.elapsed() < self.ttl {
                    return result.clone();
                }
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
}
