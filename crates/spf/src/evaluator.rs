//! Evaluator: walks a [`Record`]'s mechanisms against an IP + DNS to
//! produce a [`SpfResult`] (RFC 7208 §4).

use std::net::IpAddr;

use crate::error::{SpfError, SpfResult};
use crate::record::{Mechanism, Qualifier, Record, ip_in_subnet};
use crate::resolver::SpfResolver;

/// Inputs for an SPF verification (RFC 7208 §1.1.4).
#[derive(Debug, Clone)]
pub struct VerifyInput {
    /// IP that connected to the receiver.
    pub ip: IpAddr,
    /// HELO/EHLO domain advertised by the connecting MTA.
    pub helo: String,
    /// MAIL FROM (reverse-path) — full email address. Empty for bounces.
    pub mail_from: String,
}

impl VerifyInput {
    /// Domain to look up the SPF record at, per RFC 7208 §1.1.4:
    /// the MAIL FROM domain (or HELO if MAIL FROM is empty).
    pub fn target_domain(&self) -> &str {
        if self.mail_from.is_empty() {
            &self.helo
        } else if let Some((_, domain)) = self.mail_from.rsplit_once('@') {
            domain
        } else {
            // MAIL FROM with no @ → treat the whole thing as a domain.
            &self.mail_from
        }
    }
}

/// Maximum DNS lookups per verification per RFC 7208 §4.6.4.
const MAX_DNS_LOOKUPS: u32 = 10;
/// Cap include: recursion depth as a defense-in-depth.
const MAX_RECURSION_DEPTH: u32 = 10;

/// State carried through the evaluation: lookup counter + recursion depth.
struct EvalState {
    dns_lookups: u32,
    depth: u32,
}

impl EvalState {
    fn new() -> Self {
        Self {
            dns_lookups: 0,
            depth: 0,
        }
    }

    /// Charge a DNS lookup against the per-verification budget.
    fn charge(&mut self) -> Result<(), SpfError> {
        self.dns_lookups += 1;
        if self.dns_lookups > MAX_DNS_LOOKUPS {
            Err(SpfError::TooManyLookups)
        } else {
            Ok(())
        }
    }
}

/// Top-level SPF verification entry point.
///
/// Given a connecting IP, the connecting host's HELO domain, and the
/// envelope-From, look up + evaluate the SPF record for the
/// MAIL FROM domain. Returns one of the seven [`SpfResult`] values
/// per RFC 7208 §2.6.
///
/// ```rust,no_run
/// use mailrs_spf::{verify, VerifyInput, SpfResult, SpfResolver, SpfError};
/// use std::net::IpAddr;
/// use async_trait::async_trait;
///
/// // Your resolver — could be HickoryResolver from this crate's
/// // `hickory` feature, or your own impl.
/// # struct MyResolver;
/// # #[async_trait]
/// # impl SpfResolver for MyResolver {
/// #     async fn lookup_txt(&self, _: &str) -> Result<Vec<String>, SpfError> { Ok(vec![]) }
/// #     async fn lookup_a(&self, _: &str) -> Result<Vec<IpAddr>, SpfError> { Ok(vec![]) }
/// #     async fn lookup_aaaa(&self, _: &str) -> Result<Vec<IpAddr>, SpfError> { Ok(vec![]) }
/// #     async fn lookup_mx(&self, _: &str) -> Result<Vec<(u16, String)>, SpfError> { Ok(vec![]) }
/// # }
///
/// # async fn run() {
/// let resolver = MyResolver;
/// let input = VerifyInput {
///     ip: "203.0.113.42".parse().unwrap(),
///     helo: "mta.example.com".into(),
///     mail_from: "alice@example.com".into(),
/// };
/// let result = verify(&resolver, &input).await;
/// match result {
///     SpfResult::Pass => { /* accept */ }
///     SpfResult::Fail => { /* reject 5xx */ }
///     _ => { /* see RFC 7208 §8 for guidance */ }
/// }
/// # }
/// ```
pub async fn verify<R: SpfResolver + ?Sized>(resolver: &R, input: &VerifyInput) -> SpfResult {
    let mut state = EvalState::new();
    match verify_inner(resolver, input, input.target_domain(), &mut state).await {
        Ok(r) => r,
        Err(e) => e.to_result(),
    }
}

async fn verify_inner<R: SpfResolver + ?Sized>(
    resolver: &R,
    input: &VerifyInput,
    domain: &str,
    state: &mut EvalState,
) -> Result<SpfResult, SpfError> {
    if state.depth >= MAX_RECURSION_DEPTH {
        return Err(SpfError::TooMuchRecursion);
    }

    // Each top-level domain lookup counts as 1 (RFC 7208 §4.6.4).
    state.charge()?;
    let txts = resolver.lookup_txt(domain).await?;

    // Find the v=spf1 record. Must be exactly one (or zero = None).
    let mut spf_records: Vec<&String> = txts.iter().filter(|s| s.starts_with("v=spf1")).collect();
    if spf_records.is_empty() {
        return Ok(SpfResult::None);
    }
    if spf_records.len() > 1 {
        return Err(SpfError::MultipleRecords);
    }
    let raw = spf_records.pop().unwrap();
    let record = Record::parse(raw)?;

    eval_record(resolver, input, &record, domain, state).await
}

async fn eval_record<R: SpfResolver + ?Sized>(
    resolver: &R,
    input: &VerifyInput,
    record: &Record,
    current_domain: &str,
    state: &mut EvalState,
) -> Result<SpfResult, SpfError> {
    for mech in &record.mechanisms {
        let matched = mech_matches(resolver, input, mech, current_domain, state).await?;
        if matched {
            return Ok(qualifier_to_result(mech.qualifier()));
        }
    }
    // No mechanism matched → Neutral per RFC 7208 §4.7.
    Ok(SpfResult::Neutral)
}

fn qualifier_to_result(q: Qualifier) -> SpfResult {
    match q {
        Qualifier::Pass => SpfResult::Pass,
        Qualifier::Fail => SpfResult::Fail,
        Qualifier::SoftFail => SpfResult::SoftFail,
        Qualifier::Neutral => SpfResult::Neutral,
    }
}

/// Boxing layer so each branch resolves without infinitely-recursive
/// async (rustc rejects `async fn` recursion without indirection).
fn mech_matches<'a, R: SpfResolver + ?Sized>(
    resolver: &'a R,
    input: &'a VerifyInput,
    mech: &'a Mechanism,
    current_domain: &'a str,
    state: &'a mut EvalState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, SpfError>> + Send + 'a>> {
    Box::pin(mech_matches_impl(
        resolver,
        input,
        mech,
        current_domain,
        state,
    ))
}

async fn mech_matches_impl<R: SpfResolver + ?Sized>(
    resolver: &R,
    input: &VerifyInput,
    mech: &Mechanism,
    current_domain: &str,
    state: &mut EvalState,
) -> Result<bool, SpfError> {
    match mech {
        Mechanism::All { .. } => Ok(true),
        Mechanism::Ip4 { addr, prefix, .. } => {
            Ok(ip_in_subnet(input.ip, IpAddr::V4(*addr), *prefix))
        }
        Mechanism::Ip6 { addr, prefix, .. } => {
            Ok(ip_in_subnet(input.ip, IpAddr::V6(*addr), *prefix))
        }
        Mechanism::A {
            domain,
            ip4_prefix,
            ip6_prefix,
            ..
        } => {
            let target = domain.as_deref().unwrap_or(current_domain);
            state.charge()?;
            let ips = match input.ip {
                IpAddr::V4(_) => resolver.lookup_a(target).await?,
                IpAddr::V6(_) => resolver.lookup_aaaa(target).await?,
            };
            let prefix = match input.ip {
                IpAddr::V4(_) => *ip4_prefix,
                IpAddr::V6(_) => *ip6_prefix,
            };
            for net in ips {
                if ip_in_subnet(input.ip, net, prefix) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Mechanism::Mx {
            domain,
            ip4_prefix,
            ip6_prefix,
            ..
        } => {
            let target = domain.as_deref().unwrap_or(current_domain);
            state.charge()?;
            let mxs = resolver.lookup_mx(target).await?;
            for (_pref, mx_host) in mxs {
                state.charge()?;
                let ips = match input.ip {
                    IpAddr::V4(_) => resolver.lookup_a(&mx_host).await?,
                    IpAddr::V6(_) => resolver.lookup_aaaa(&mx_host).await?,
                };
                let prefix = match input.ip {
                    IpAddr::V4(_) => *ip4_prefix,
                    IpAddr::V6(_) => *ip6_prefix,
                };
                for net in ips {
                    if ip_in_subnet(input.ip, net, prefix) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        Mechanism::Include { domain, .. } => {
            // Recurse — the included record is evaluated as its own
            // sub-SPF. RFC 7208 §5.2: only `Pass` from the included
            // record counts as a match for this mechanism.
            state.depth += 1;
            let sub = verify_inner(resolver, input, domain, state).await?;
            state.depth -= 1;
            Ok(matches!(sub, SpfResult::Pass))
        }
        Mechanism::Exists { domain, .. } => {
            // RFC 7208 §5.7: any A record for the resolved name = match.
            // We don't expand macros in v1.0 (out of scope); we look up
            // the literal name.
            state.charge()?;
            let ips = resolver.lookup_a(domain).await?;
            Ok(!ips.is_empty())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::SpfResolver;
    use async_trait::async_trait;
    use std::collections::HashMap;

    /// Fake resolver: deterministic, in-memory.
    #[derive(Default)]
    struct FakeResolver {
        txt: HashMap<String, Vec<String>>,
        a: HashMap<String, Vec<IpAddr>>,
        aaaa: HashMap<String, Vec<IpAddr>>,
        mx: HashMap<String, Vec<(u16, String)>>,
    }

    impl FakeResolver {
        fn with_txt(mut self, domain: &str, records: Vec<&str>) -> Self {
            self.txt.insert(
                domain.into(),
                records.into_iter().map(String::from).collect(),
            );
            self
        }
        fn with_a(mut self, domain: &str, ips: Vec<&str>) -> Self {
            self.a.insert(
                domain.into(),
                ips.into_iter().map(|s| s.parse().unwrap()).collect(),
            );
            self
        }
        fn with_mx(mut self, domain: &str, mxs: Vec<(u16, &str)>) -> Self {
            self.mx.insert(
                domain.into(),
                mxs.into_iter().map(|(p, h)| (p, h.into())).collect(),
            );
            self
        }
    }

    #[async_trait]
    impl SpfResolver for FakeResolver {
        async fn lookup_txt(&self, d: &str) -> Result<Vec<String>, SpfError> {
            Ok(self.txt.get(d).cloned().unwrap_or_default())
        }
        async fn lookup_a(&self, d: &str) -> Result<Vec<IpAddr>, SpfError> {
            Ok(self.a.get(d).cloned().unwrap_or_default())
        }
        async fn lookup_aaaa(&self, d: &str) -> Result<Vec<IpAddr>, SpfError> {
            Ok(self.aaaa.get(d).cloned().unwrap_or_default())
        }
        async fn lookup_mx(&self, d: &str) -> Result<Vec<(u16, String)>, SpfError> {
            Ok(self.mx.get(d).cloned().unwrap_or_default())
        }
    }

    fn input(ip: &str, helo: &str, mail_from: &str) -> VerifyInput {
        VerifyInput {
            ip: ip.parse().unwrap(),
            helo: helo.into(),
            mail_from: mail_from.into(),
        }
    }

    #[tokio::test]
    async fn no_spf_record_yields_none() {
        let r = FakeResolver::default();
        let res = verify(
            &r,
            &input("1.2.3.4", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::None);
    }

    #[tokio::test]
    async fn matching_ip4_yields_pass() {
        let r =
            FakeResolver::default().with_txt("example.com", vec!["v=spf1 ip4:203.0.113.0/24 -all"]);
        let res = verify(
            &r,
            &input("203.0.113.42", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Pass);
    }

    #[tokio::test]
    async fn non_matching_ip_with_minus_all_yields_fail() {
        let r =
            FakeResolver::default().with_txt("example.com", vec!["v=spf1 ip4:203.0.113.0/24 -all"]);
        let res = verify(
            &r,
            &input("198.51.100.5", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Fail);
    }

    #[tokio::test]
    async fn non_matching_ip_with_tilde_all_yields_softfail() {
        let r =
            FakeResolver::default().with_txt("example.com", vec!["v=spf1 ip4:203.0.113.0/24 ~all"]);
        let res = verify(
            &r,
            &input("198.51.100.5", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::SoftFail);
    }

    #[tokio::test]
    async fn empty_mail_from_uses_helo_domain() {
        let r = FakeResolver::default()
            .with_txt("mta.example.com", vec!["v=spf1 ip4:203.0.113.0/24 -all"]);
        // mail_from empty (bounce); helo is the domain.
        let res = verify(&r, &input("203.0.113.42", "mta.example.com", "")).await;
        assert_eq!(res, SpfResult::Pass);
    }

    #[tokio::test]
    async fn a_mechanism_matches_via_dns() {
        let r = FakeResolver::default()
            .with_txt("example.com", vec!["v=spf1 a -all"])
            .with_a("example.com", vec!["203.0.113.42"]);
        let res = verify(
            &r,
            &input("203.0.113.42", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Pass);
    }

    #[tokio::test]
    async fn mx_mechanism_matches_via_dns() {
        let r = FakeResolver::default()
            .with_txt("example.com", vec!["v=spf1 mx -all"])
            .with_mx("example.com", vec![(10, "mx1.example.com")])
            .with_a("mx1.example.com", vec!["203.0.113.10"]);
        let res = verify(
            &r,
            &input("203.0.113.10", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Pass);
    }

    #[tokio::test]
    async fn include_recurses_and_pass_propagates_match() {
        let r = FakeResolver::default()
            .with_txt("example.com", vec!["v=spf1 include:_spf.partner.com -all"])
            .with_txt("_spf.partner.com", vec!["v=spf1 ip4:203.0.113.0/24 -all"]);
        let res = verify(
            &r,
            &input("203.0.113.42", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Pass);
    }

    #[tokio::test]
    async fn include_fail_does_not_match() {
        // include: only Pass = match. Anything else doesn't.
        let r = FakeResolver::default()
            .with_txt("example.com", vec!["v=spf1 include:strict.com ~all"])
            .with_txt("strict.com", vec!["v=spf1 -all"]);
        let res = verify(
            &r,
            &input("203.0.113.42", "mta.example.com", "alice@example.com"),
        )
        .await;
        // include yields Fail → not a match → fall through to ~all → SoftFail
        assert_eq!(res, SpfResult::SoftFail);
    }

    #[tokio::test]
    async fn multiple_v_spf1_yields_permerror() {
        let r = FakeResolver::default().with_txt("example.com", vec!["v=spf1 -all", "v=spf1 +all"]);
        let res = verify(
            &r,
            &input("1.2.3.4", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::PermError);
    }

    #[tokio::test]
    async fn target_domain_helo_when_mail_from_empty() {
        let i = input("1.2.3.4", "mta.example.com", "");
        assert_eq!(i.target_domain(), "mta.example.com");
    }

    #[tokio::test]
    async fn target_domain_mail_from_part() {
        let i = input("1.2.3.4", "mta.example.com", "alice@example.com");
        assert_eq!(i.target_domain(), "example.com");
    }

    #[tokio::test]
    async fn no_match_no_all_yields_neutral() {
        // Record with ip4 only and no `all` — no match → Neutral per §4.7
        let r = FakeResolver::default().with_txt("example.com", vec!["v=spf1 ip4:203.0.113.0/24"]);
        let res = verify(
            &r,
            &input("198.51.100.5", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Neutral);
    }

    #[tokio::test]
    async fn ipv6_match_works() {
        let r =
            FakeResolver::default().with_txt("example.com", vec!["v=spf1 ip6:2001:db8::/32 -all"]);
        let res = verify(
            &r,
            &input("2001:db8::1", "mta.example.com", "alice@example.com"),
        )
        .await;
        assert_eq!(res, SpfResult::Pass);
    }
}
