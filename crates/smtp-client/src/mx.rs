use std::collections::HashMap;
use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use hickory_resolver::TokioResolver;

/// MX record with priority
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MxRecord {
    pub priority: u16,
    pub exchange: String,
}

/// resolve MX records for a domain, falling back to A/AAAA if none found
pub async fn resolve_mx(
    resolver: &TokioResolver,
    domain: &str,
) -> io::Result<Vec<MxRecord>> {
    if domain.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty domain",
        ));
    }

    use hickory_resolver::proto::rr::RData;
    match resolver.mx_lookup(domain).await {
        Ok(mx_response) => {
            let mut records: Vec<MxRecord> = mx_response
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::MX(mx) => {
                        let exchange = mx.exchange.to_utf8();
                        // remove trailing dot from FQDN
                        let exchange = exchange.trim_end_matches('.').to_string();
                        Some(MxRecord {
                            priority: mx.preference,
                            exchange,
                        })
                    }
                    _ => None,
                })
                .collect();

            if records.is_empty() {
                return Ok(fallback_to_domain(domain));
            }

            sort_mx_records(&mut records);
            Ok(records)
        }
        Err(_) => {
            // no MX records found — fall back to domain itself (RFC 5321 section 5.1)
            Ok(fallback_to_domain(domain))
        }
    }
}

/// when no MX records exist, use the domain itself as the mail exchange
pub fn fallback_to_domain(domain: &str) -> Vec<MxRecord> {
    vec![MxRecord {
        priority: 0,
        exchange: domain.to_string(),
    }]
}

/// sort MX records: lowest priority first, alphabetical tiebreak
pub fn sort_mx_records(records: &mut [MxRecord]) {
    records.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.exchange.cmp(&b.exchange))
    });
}

/// format MAIL FROM command (sanitizes address to prevent SMTP injection)
pub fn format_mail_from(sender: &str) -> String {
    let safe = sanitize_address(sender);
    format!("MAIL FROM:<{safe}>\r\n")
}

/// format RCPT TO command (sanitizes address to prevent SMTP injection)
pub fn format_rcpt_to(recipient: &str) -> String {
    let safe = sanitize_address(recipient);
    format!("RCPT TO:<{safe}>\r\n")
}

/// cached MX resolver to avoid repeated DNS queries
pub struct MxCache {
    cache: Mutex<HashMap<String, (Vec<MxRecord>, Instant)>>,
    ttl: Duration,
}

impl MxCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// resolve with cache: return cached result if fresh, otherwise query DNS
    pub async fn resolve(
        &self,
        resolver: &TokioResolver,
        domain: &str,
    ) -> io::Result<Vec<MxRecord>> {
        // check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some((records, inserted_at)) = cache.get(domain)
                && inserted_at.elapsed() < self.ttl {
                    return Ok(records.clone());
                }
        }

        // cache miss or expired — resolve
        let records = resolve_mx(resolver, domain).await?;

        // store in cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(domain.to_string(), (records.clone(), Instant::now()));
        }

        Ok(records)
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

/// strip characters that could cause SMTP command injection
fn sanitize_address(addr: &str) -> String {
    addr.chars()
        .filter(|c| *c != '>' && *c != '\r' && *c != '\n')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_by_priority() {
        let mut records = vec![
            MxRecord {
                priority: 20,
                exchange: "mx2.example.com".into(),
            },
            MxRecord {
                priority: 10,
                exchange: "mx1.example.com".into(),
            },
            MxRecord {
                priority: 30,
                exchange: "mx3.example.com".into(),
            },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].priority, 10);
        assert_eq!(records[1].priority, 20);
        assert_eq!(records[2].priority, 30);
    }

    #[test]
    fn tiebreak_alphabetical() {
        let mut records = vec![
            MxRecord {
                priority: 10,
                exchange: "mx-b.example.com".into(),
            },
            MxRecord {
                priority: 10,
                exchange: "mx-a.example.com".into(),
            },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].exchange, "mx-a.example.com");
        assert_eq!(records[1].exchange, "mx-b.example.com");
    }

    #[test]
    fn format_mail_from_cmd() {
        assert_eq!(
            format_mail_from("sender@example.com"),
            "MAIL FROM:<sender@example.com>\r\n"
        );
    }

    #[test]
    fn format_rcpt_to_cmd() {
        assert_eq!(
            format_rcpt_to("rcpt@example.com"),
            "RCPT TO:<rcpt@example.com>\r\n"
        );
    }

    #[test]
    fn empty_records() {
        let mut records: Vec<MxRecord> = vec![];
        sort_mx_records(&mut records);
        assert!(records.is_empty());
    }

    #[test]
    fn resolve_mx_fallback_to_a() {
        let records = fallback_to_domain("example.com");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].priority, 0);
        assert_eq!(records[0].exchange, "example.com");
    }

    #[test]
    fn mx_record_from_dns_data() {
        let record = MxRecord {
            priority: 10,
            exchange: "mx1.example.com".into(),
        };
        assert_eq!(record.priority, 10);
        assert_eq!(record.exchange, "mx1.example.com");

        let record2 = MxRecord {
            priority: 20,
            exchange: "mx2.example.com".into(),
        };
        let mut records = vec![record2.clone(), record.clone()];
        sort_mx_records(&mut records);
        assert_eq!(records[0].exchange, "mx1.example.com");
        assert_eq!(records[1].exchange, "mx2.example.com");
    }

    #[tokio::test]
    async fn resolve_mx_empty_domain_error() {
        let resolver = TokioResolver::builder_tokio()
            .expect("failed to create resolver builder")
            .build()
            .expect("failed to build resolver");
        let result = resolve_mx(&resolver, "").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn mx_cache_hit() {
        let cache = MxCache::new(Duration::from_secs(300));
        let records = vec![MxRecord {
            priority: 10,
            exchange: "mx.example.com".into(),
        }];

        // manually insert into cache
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(
                "example.com".into(),
                (records.clone(), Instant::now()),
            );
        }

        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn mx_cache_expired() {
        let cache = MxCache::new(Duration::from_millis(1));
        let records = vec![MxRecord {
            priority: 10,
            exchange: "mx.example.com".into(),
        }];

        // insert with a past instant
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(
                "example.com".into(),
                (records, Instant::now() - Duration::from_secs(10)),
            );
        }

        // cleanup should remove expired
        cache.cleanup();
        assert_eq!(cache.len(), 0);
    }

    // --- additional tests ---

    #[test]
    fn sanitize_strips_angle_bracket() {
        assert_eq!(
            format_mail_from("user>injected@example.com"),
            "MAIL FROM:<userinjected@example.com>\r\n"
        );
    }

    #[test]
    fn sanitize_strips_crlf_injection() {
        // attempt SMTP injection via \r\n in address
        assert_eq!(
            format_rcpt_to("user@evil.com\r\nDATA"),
            "RCPT TO:<user@evil.comDATA>\r\n"
        );
    }

    #[test]
    fn sanitize_strips_newline_only() {
        assert_eq!(
            format_mail_from("user@evil.com\nDATA"),
            "MAIL FROM:<user@evil.comDATA>\r\n"
        );
    }

    #[test]
    fn sanitize_strips_carriage_return_only() {
        assert_eq!(
            format_rcpt_to("user@evil.com\rDATA"),
            "RCPT TO:<user@evil.comDATA>\r\n"
        );
    }

    #[test]
    fn format_mail_from_empty() {
        // empty sender (bounce address)
        assert_eq!(format_mail_from(""), "MAIL FROM:<>\r\n");
    }

    #[test]
    fn format_rcpt_to_normal() {
        assert_eq!(
            format_rcpt_to("alice@example.com"),
            "RCPT TO:<alice@example.com>\r\n"
        );
    }

    #[test]
    fn fallback_to_domain_preserves_case() {
        let records = fallback_to_domain("Example.COM");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].exchange, "Example.COM");
        assert_eq!(records[0].priority, 0);
    }

    #[test]
    fn sort_mx_records_single_record() {
        let mut records = vec![MxRecord {
            priority: 42,
            exchange: "mx.example.com".into(),
        }];
        sort_mx_records(&mut records);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].priority, 42);
    }

    #[test]
    fn sort_mx_records_already_sorted() {
        let mut records = vec![
            MxRecord { priority: 10, exchange: "a.example.com".into() },
            MxRecord { priority: 20, exchange: "b.example.com".into() },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].exchange, "a.example.com");
        assert_eq!(records[1].exchange, "b.example.com");
    }

    #[test]
    fn sort_mx_records_reverse_order() {
        let mut records = vec![
            MxRecord { priority: 50, exchange: "z.example.com".into() },
            MxRecord { priority: 30, exchange: "m.example.com".into() },
            MxRecord { priority: 10, exchange: "a.example.com".into() },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].priority, 10);
        assert_eq!(records[1].priority, 30);
        assert_eq!(records[2].priority, 50);
    }

    #[test]
    fn sort_mx_records_same_priority_multiple() {
        let mut records = vec![
            MxRecord { priority: 10, exchange: "c.example.com".into() },
            MxRecord { priority: 10, exchange: "a.example.com".into() },
            MxRecord { priority: 10, exchange: "b.example.com".into() },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].exchange, "a.example.com");
        assert_eq!(records[1].exchange, "b.example.com");
        assert_eq!(records[2].exchange, "c.example.com");
    }

    #[test]
    fn mx_cache_new_is_empty() {
        let cache = MxCache::new(Duration::from_secs(60));
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn mx_cache_cleanup_keeps_fresh() {
        let cache = MxCache::new(Duration::from_secs(300));
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(
                "fresh.com".into(),
                (vec![MxRecord { priority: 10, exchange: "mx.fresh.com".into() }], Instant::now()),
            );
            c.insert(
                "stale.com".into(),
                (
                    vec![MxRecord { priority: 10, exchange: "mx.stale.com".into() }],
                    Instant::now() - Duration::from_secs(600),
                ),
            );
        }
        assert_eq!(cache.len(), 2);
        cache.cleanup();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn mx_cache_multiple_domains() {
        let cache = MxCache::new(Duration::from_secs(300));
        {
            let mut c = cache.cache.lock().unwrap();
            for i in 0..5 {
                c.insert(
                    format!("domain{i}.com"),
                    (
                        vec![MxRecord {
                            priority: 10,
                            exchange: format!("mx.domain{i}.com"),
                        }],
                        Instant::now(),
                    ),
                );
            }
        }
        assert_eq!(cache.len(), 5);
    }

    #[test]
    fn mx_record_equality() {
        let a = MxRecord { priority: 10, exchange: "mx.example.com".into() };
        let b = MxRecord { priority: 10, exchange: "mx.example.com".into() };
        let c = MxRecord { priority: 20, exchange: "mx.example.com".into() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn mx_record_debug() {
        let r = MxRecord { priority: 10, exchange: "mx.test.com".into() };
        let debug = format!("{:?}", r);
        assert!(debug.contains("10"));
        assert!(debug.contains("mx.test.com"));
    }

    #[test]
    fn format_mail_from_with_plus_addressing() {
        assert_eq!(
            format_mail_from("user+tag@example.com"),
            "MAIL FROM:<user+tag@example.com>\r\n"
        );
    }

    #[test]
    fn format_rcpt_to_with_dots() {
        assert_eq!(
            format_rcpt_to("first.last@example.com"),
            "RCPT TO:<first.last@example.com>\r\n"
        );
    }

    // --- new tests ---

    #[test]
    fn sanitize_strips_multiple_angle_brackets() {
        assert_eq!(
            format_mail_from("u>s>e>r@example.com"),
            "MAIL FROM:<user@example.com>\r\n"
        );
    }

    #[test]
    fn sanitize_strips_combined_injection() {
        // combined \r\n and > in one address
        assert_eq!(
            format_rcpt_to("bad>\r\nRCPT TO:<evil@x.com"),
            "RCPT TO:<badRCPT TO:<evil@x.com>\r\n"
        );
    }

    #[test]
    fn format_mail_from_unicode_address() {
        // international email addresses should pass through (minus dangerous chars)
        assert_eq!(
            format_mail_from("用户@example.com"),
            "MAIL FROM:<用户@example.com>\r\n"
        );
    }

    #[test]
    fn format_rcpt_to_empty_address() {
        assert_eq!(format_rcpt_to(""), "RCPT TO:<>\r\n");
    }

    #[test]
    fn sort_mx_records_mixed_priorities_and_names() {
        let mut records = vec![
            MxRecord { priority: 20, exchange: "b.example.com".into() },
            MxRecord { priority: 10, exchange: "z.example.com".into() },
            MxRecord { priority: 20, exchange: "a.example.com".into() },
            MxRecord { priority: 10, exchange: "a.example.com".into() },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0], MxRecord { priority: 10, exchange: "a.example.com".into() });
        assert_eq!(records[1], MxRecord { priority: 10, exchange: "z.example.com".into() });
        assert_eq!(records[2], MxRecord { priority: 20, exchange: "a.example.com".into() });
        assert_eq!(records[3], MxRecord { priority: 20, exchange: "b.example.com".into() });
    }

    #[test]
    fn sort_mx_records_priority_zero() {
        let mut records = vec![
            MxRecord { priority: 10, exchange: "mx1.example.com".into() },
            MxRecord { priority: 0, exchange: "mx0.example.com".into() },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].priority, 0);
        assert_eq!(records[1].priority, 10);
    }

    #[test]
    fn sort_mx_records_max_priority() {
        let mut records = vec![
            MxRecord { priority: u16::MAX, exchange: "low.example.com".into() },
            MxRecord { priority: 0, exchange: "high.example.com".into() },
        ];
        sort_mx_records(&mut records);
        assert_eq!(records[0].priority, 0);
        assert_eq!(records[1].priority, u16::MAX);
    }

    #[test]
    fn fallback_to_domain_empty_string() {
        let records = fallback_to_domain("");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].exchange, "");
        assert_eq!(records[0].priority, 0);
    }

    #[test]
    fn fallback_to_domain_subdomain() {
        let records = fallback_to_domain("mail.sub.example.com");
        assert_eq!(records[0].exchange, "mail.sub.example.com");
    }

    #[test]
    fn mx_cache_overwrite_same_domain() {
        let cache = MxCache::new(Duration::from_secs(300));
        {
            let mut c = cache.cache.lock().unwrap();
            c.insert(
                "example.com".into(),
                (vec![MxRecord { priority: 10, exchange: "old.example.com".into() }], Instant::now()),
            );
            // overwrite with new records
            c.insert(
                "example.com".into(),
                (vec![MxRecord { priority: 10, exchange: "new.example.com".into() }], Instant::now()),
            );
        }
        assert_eq!(cache.len(), 1);
        let c = cache.cache.lock().unwrap();
        assert_eq!(c.get("example.com").unwrap().0[0].exchange, "new.example.com");
    }

    #[test]
    fn mx_cache_cleanup_all_expired() {
        let cache = MxCache::new(Duration::from_millis(1));
        {
            let mut c = cache.cache.lock().unwrap();
            let past = Instant::now() - Duration::from_secs(10);
            c.insert("a.com".into(), (vec![], past));
            c.insert("b.com".into(), (vec![], past));
            c.insert("c.com".into(), (vec![], past));
        }
        assert_eq!(cache.len(), 3);
        cache.cleanup();
        assert!(cache.is_empty());
    }

    #[test]
    fn mx_cache_cleanup_empty_is_noop() {
        let cache = MxCache::new(Duration::from_secs(60));
        cache.cleanup();
        assert!(cache.is_empty());
    }

    #[test]
    fn mx_record_clone() {
        let original = MxRecord { priority: 10, exchange: "mx.example.com".into() };
        let cloned = original.clone();
        assert_eq!(original, cloned);
        // ensure clone is independent
        assert_eq!(cloned.priority, 10);
        assert_eq!(cloned.exchange, "mx.example.com");
    }
}
