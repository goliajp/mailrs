use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use mailrs_spf::{Record, SpfError, SpfResolver, VerifyInput};
use std::collections::HashMap;
use std::hint::black_box;
use std::net::IpAddr;

fn bench_parse_simple(c: &mut Criterion) {
    c.bench_function("parse/v=spf1_ip4_minus_all", |b| {
        b.iter(|| {
            let r = Record::parse(black_box("v=spf1 ip4:203.0.113.0/24 -all"));
            black_box(r.unwrap())
        });
    });
}

fn bench_parse_complex(c: &mut Criterion) {
    let input = "v=spf1 ip4:203.0.113.0/24 ip4:198.51.100.0/24 ip6:2001:db8::/32 a:mail.example.com mx:example.com include:_spf.google.com include:spf.protection.outlook.com -all";
    c.bench_function("parse/complex_record_8_mechanisms", |b| {
        b.iter(|| {
            let r = Record::parse(black_box(input));
            black_box(r.unwrap())
        });
    });
}

/// Fake resolver for evaluator bench (no network).
struct StaticResolver;

#[async_trait]
impl SpfResolver for StaticResolver {
    async fn lookup_txt(&self, d: &str) -> Result<Vec<String>, SpfError> {
        Ok(match d {
            "example.com" => vec!["v=spf1 ip4:203.0.113.0/24 -all".into()],
            _ => vec![],
        })
    }
    async fn lookup_a(&self, _: &str) -> Result<Vec<IpAddr>, SpfError> {
        Ok(vec![])
    }
    async fn lookup_aaaa(&self, _: &str) -> Result<Vec<IpAddr>, SpfError> {
        Ok(vec![])
    }
    async fn lookup_mx(&self, _: &str) -> Result<Vec<(u16, String)>, SpfError> {
        Ok(vec![])
    }
}

fn bench_verify_pass(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let resolver = StaticResolver;
    let input = VerifyInput {
        ip: "203.0.113.42".parse().unwrap(),
        helo: "mta.example.com".into(),
        mail_from: "alice@example.com".into(),
    };
    c.bench_function("verify/pass_path_no_real_dns", |b| {
        b.iter(|| {
            let r = rt.block_on(mailrs_spf::verify(black_box(&resolver), black_box(&input)));
            black_box(r)
        });
    });
}

// Dummy to avoid unused-import warnings from HashMap
fn _suppress_unused() {
    let _ = HashMap::<String, String>::new();
}

criterion_group!(
    benches,
    bench_parse_simple,
    bench_parse_complex,
    bench_verify_pass
);
criterion_main!(benches);
