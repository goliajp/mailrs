//! Checks on the client IP alone, made before the greeting.
//!
//! The admin's networks come first. A black network is refused the way a
//! DNSBL listing is, with 554 in place of the greeting; a white one skips
//! the rate limit and DNSBL. Address and domain entries cannot be read
//! here: no sender is known and nothing is authenticated until DATA, where
//! `inbound::stages::local_lists` reads every kind.

use std::net::IpAddr;

use hickory_resolver::TokioResolver;
use mailrs_smtp_proto::response::Response;

use super::ConnectionContext;
use crate::greylist_local::GreylistLocalHandle;
use crate::inbound::rate_limit::RateLimitStore;

pub(crate) struct ConnectChecks<'a> {
    pub lists: &'a GreylistLocalHandle,
    pub rate_limiter: &'a dyn RateLimitStore,
    /// The resolver and zones, when DNSBL is on.
    pub dnsbl: Option<(&'a TokioResolver, &'a [String])>,
}

/// The reply sent in place of the greeting, and the reason the live
/// monitor shows.
#[derive(Debug)]
pub(crate) struct Refusal {
    pub reply: String,
    pub reason: String,
}

impl<'a> ConnectChecks<'a> {
    pub(crate) fn of(ctx: &'a ConnectionContext) -> Self {
        let dnsbl = match (
            &ctx.resolver,
            ctx.dnsbl_enabled && !ctx.dnsbl_zones.is_empty(),
        ) {
            (Some(r), true) => Some((r.as_ref(), ctx.dnsbl_zones.as_slice())),
            _ => None,
        };
        Self {
            lists: &ctx.runtime.sender_lists,
            rate_limiter: ctx.rate_limiter.as_ref(),
            dnsbl,
        }
    }

    pub(crate) async fn refusal(&self, ip: IpAddr) -> Option<Refusal> {
        let (black, white) = {
            let lists = self.lists.read().await;
            // an empty sender leaves only the network entries to match
            (
                lists.matches_black("", ip).is_some(),
                lists.white_network(ip),
            )
        };
        if black {
            metrics::counter!("mailrs_greylist_local_black_hit_total", "kind" => "cidr_connect")
                .increment(1);
            return Some(Refusal {
                reply: format!(
                    "554 5.7.1 Service unavailable; client [{ip}] blocked by local policy\r\n"
                ),
                reason: "local black network".into(),
            });
        }
        if white {
            metrics::counter!("mailrs_greylist_local_white_hit_total", "kind" => "cidr_connect")
                .increment(1);
            return None;
        }
        if !self.rate_limiter.check(&ip.to_string()).await {
            return Some(Refusal {
                reply: Response::rate_limited().format(),
                reason: "rate limited".into(),
            });
        }
        let (resolver, zones) = self.dnsbl?;
        let (zone, result) = mailrs_shield::dnsbl::check_dnsbl(resolver, ip, zones).await?;
        Some(Refusal {
            reply: format!(
                "554 5.7.1 Service unavailable; client [{ip}] blocked using {zone} ({result:?})\r\n"
            ),
            reason: format!("DNSBL: {zone} ({result:?})"),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;
    use std::sync::Arc;

    use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig};
    use hickory_resolver::net::runtime::TokioRuntimeProvider;
    use hickory_resolver::proto::op::{Message, OpCode};
    use hickory_resolver::proto::rr::{RData, Record, rdata::A};
    use ipnet::IpNet;
    use mailrs_rate_limit::{InMemoryRateLimitStore, TokenBucketConfig};

    use super::*;
    use crate::greylist_local::GreylistLocalLists;

    const WHITE: &str = "203.0.113.9";
    const BLACK: &str = "192.0.2.9";
    const OTHER: &str = "198.51.100.7";

    fn lists() -> GreylistLocalHandle {
        let mut l = GreylistLocalLists::default();
        l.white_cidrs
            .push(IpNet::from_str("203.0.113.0/24").unwrap());
        // black inside a white network: black wins
        l.white_cidrs.push(IpNet::from_str("192.0.2.0/24").unwrap());
        l.black_cidrs.push(IpNet::from_str("192.0.2.9/32").unwrap());
        Arc::new(tokio::sync::RwLock::new(l))
    }

    // one connection, then none for a very long time
    fn one_connection_each() -> InMemoryRateLimitStore {
        InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0001,
        })
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn a_white_network_is_not_rate_limited() {
        let lists = lists();
        let rate = one_connection_each();
        let checks = ConnectChecks {
            lists: &lists,
            rate_limiter: &rate,
            dnsbl: None,
        };
        assert!(checks.refusal(ip(OTHER)).await.is_none());
        let limited = checks.refusal(ip(OTHER)).await.expect("second is limited");
        assert!(limited.reply.starts_with("421 "), "{limited:?}");
        for _ in 0..3 {
            assert!(checks.refusal(ip(WHITE)).await.is_none());
        }
    }

    #[tokio::test]
    async fn a_black_network_is_refused_before_the_greeting() {
        let lists = lists();
        let rate = one_connection_each();
        let checks = ConnectChecks {
            lists: &lists,
            rate_limiter: &rate,
            dnsbl: None,
        };
        let refused = checks.refusal(ip(BLACK)).await.expect("refused");
        assert!(refused.reply.starts_with("554 5.7.1 "), "{refused:?}");
        assert!(refused.reply.ends_with("\r\n"));
        // the rest of its white network is not
        assert!(checks.refusal(ip("192.0.2.10")).await.is_none());
    }

    /// A name server on loopback that lists every address it is asked
    /// about (127.0.0.2, the "listed" answer).
    async fn listing_everything() -> TokioResolver {
        let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = sock.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                let (n, from) = sock.recv_from(&mut buf).await.unwrap();
                let query = Message::from_vec(&buf[..n]).unwrap();
                let mut reply = Message::response(query.metadata.id, OpCode::Query);
                reply.metadata.recursion_desired = query.metadata.recursion_desired;
                reply.metadata.recursion_available = true;
                for q in &query.queries {
                    reply.add_answer(Record::from_rdata(
                        q.name().clone(),
                        60,
                        RData::A(A(Ipv4Addr::new(127, 0, 0, 2))),
                    ));
                }
                reply.add_queries(query.queries.clone());
                sock.send_to(&reply.to_vec().unwrap(), from).await.unwrap();
            }
        });
        let mut udp = ConnectionConfig::udp();
        udp.port = port;
        let ns = NameServerConfig::new(ip("127.0.0.1"), true, vec![udp]);
        let config = ResolverConfig::from_parts(None, vec![], vec![ns]);
        TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn a_white_network_is_not_checked_against_dnsbl() {
        let resolver = listing_everything().await;
        let zones = vec!["zen.example".to_string()];
        let lists = lists();
        let rate = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        let checks = ConnectChecks {
            lists: &lists,
            rate_limiter: &rate,
            dnsbl: Some((&resolver, &zones)),
        };
        let listed = checks.refusal(ip(OTHER)).await.expect("DNSBL hit");
        assert!(listed.reply.starts_with("554 5.7.1 "), "{listed:?}");
        assert!(
            listed.reason.starts_with("DNSBL: zen.example"),
            "{listed:?}"
        );
        assert!(checks.refusal(ip(WHITE)).await.is_none());
    }
}
