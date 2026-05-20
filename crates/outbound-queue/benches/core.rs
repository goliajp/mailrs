//! Micro-benchmarks for outbound-queue hot paths.
//!
//! Run with: `cargo bench -p mailrs-outbound-queue`.
//!
//! Note: the DKIM key is a throwaway test key embedded for repeatable
//! signing benchmarks. **Never use it for anything else.**

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_outbound_queue::dkim_sign::DkimSignConfig;
use mailrs_outbound_queue::retry::{retry_delay_secs, should_bounce};

/// Test-only RSA 2048-bit private key, PKCS#8 PEM. Generated locally for
/// benchmark repeatability — DO NOT use in production.
const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEuwIBADANBgkqhkiG9w0BAQEFAASCBKUwggShAgEAAoIBAQC8tKt07tAVI976
ZR56lbcFS83+Zmm/Z21dyAhBbjXKQEBbIQ9ZRQ5kRP3Saj0z22gYMAk8n+Un31Ut
rCZ2VyjnucbAdNzotcLwvknlEgDk1ZrMV3sohyROnaroCnY8NJEaH2vBRD+D60pM
Kgbm6lhZZXWtrG7/T0TMX4rpdeaD9LtXNZmER1f5xP7I3cAt7dOhdH6or+geCqLv
hv6ceO0/O2d51gLAjqwdR6bL607aXSwRh/Kj4DaQczdOeNnryBxyXp8RrDncgRzA
hKMBcUeUz2WMZ7h0T4VsfU8hk1g6D092xaTNTJyui0hEqrIfn8ohW5HvGhisvVqq
unCNxkohAgMBAAECggEADk+8H9PWJNRMnpGOsd4APGmXB4U9iI5P2r3x9QG7w0lY
2CXLY997NkWOjn0Pg+BO+7lLlLUiYazBaVkArcGce5Pjq/Lw/MEpi1fog3P4bLfP
0/w1KJ9fNIsmoWSHD7fY4rH2r8387ISqui4HH0+EldBpEAaKKy4YL1EhAuW12X9T
lch+zKArtjvGQf2iNEA0QJN+VzJppR+E+ILdreMvEHNRO9DFRqDYzSYFmRzKv+yz
mCgR6AdpZ/V/VWsli9W27f+ZzCjtfDY9DEDrVEkZBenzlKIGwuKpG6P5tvG2eBKx
/GzOqEJfCdKsXE41ZjDX5vcAik3GbwMsiMFTE5UfcQKBgQDwj78kh+HDkhy2PtMw
eD1Ccd3eiovkdTAxhq4nJX8tDoV+nT5k7fMdOH40xDeXowSm+oKKo628FyO28skw
v/U90Pd5f9mASAYNb9Fd0Bw2PYRsfI1bZrVVMNCUnFXTozYUUZrj2qVno0KtLj3r
7wK5HPYEjEpHt6fX7J4OafS1hQKBgQDI0PhOkt0FXsX6DolS4zeAyLpFiQtv5ZHm
r6z3XALIrFa1jiYlG3tT7+vUc0tWjgYMw1MbmIABmwAMKnTX3txkpUo+1jyauJDm
6NUUVnIipXIBfvQhXNkCJ4gfyNzAgFUHwOnV0kWWuRh0rsQtXo+rlAQyptWdlTfN
beyGsk6m7QKBgQCF8sSwBqmDSHyMTfcyagFSWiz8mZDDqS0opqGaUpq57/gNRGlV
sdlJUAeWQhviZ3dTsvG1WOaIcSoF2LKGXpyjyxPBp5rofzI/kR+3aQYMfbya28+q
MUqPIRtDZLm1mo+mSLpCXaD0UEf0PmdkVDXj9WhXp/ZEcNMYvDxWMlF8MQJ/FfbF
MdLeWbgD71Wnr2kqqOtLdE+I8LQrQQ/12xg1Nb2jvjfN9EENPCEBqjryAoWGI985
N8t2NLa/SpVaMkIt2NQ7SqQj/MgzEQ5mP9M0qJVv4rn/+aYuFg481T0i5+shYbe3
26sj8VhNVHXI/y1YiWunCeM1egbYE5/yPslB0QKBgDEKwthV2q7/O4+vxCHC2uGp
7Kz3IwlRX3OnLI+n/fPmDx8R633SB0jAH3KZA2aQPZEooXTS5ixWJB4K7sKQ6iS1
Qu+fEcMA+Psq04Zz22FxPas6ItHc8mu9WFYV8Qp2LiKPyJAezUxCjPrFBuNwwwKJ
NINFsfz84DYbHa6wYcBa
-----END PRIVATE KEY-----";

const SHORT_MESSAGE: &[u8] = b"From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: Hello\r\n\
Date: Wed, 20 May 2026 12:00:00 +0900\r\n\
Message-ID: <test1@example.com>\r\n\
\r\n\
Hello, world!\r\n";

const LONG_MESSAGE_TEMPLATE: &str = "From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: Long message body\r\n\
Date: Wed, 20 May 2026 12:00:00 +0900\r\n\
Message-ID: <test2@example.com>\r\n\
\r\n\
{}\r\n";

fn bench_dkim_sign(c: &mut Criterion) {
    let cfg = DkimSignConfig {
        selector: "test".into(),
        domain: "example.com".into(),
        private_key_pem: TEST_KEY_PEM.into(),
    };

    let mut group = c.benchmark_group("dkim_sign");
    group.bench_function("short", |b| b.iter(|| cfg.sign(black_box(SHORT_MESSAGE))));

    let body = "Lorem ipsum dolor sit amet, ".repeat(200);
    let long = LONG_MESSAGE_TEMPLATE.replace("{}", &body).into_bytes();
    group.bench_function("long_8kb", |b| b.iter(|| cfg.sign(black_box(&long))));
    group.finish();
}

fn bench_retry_policy(c: &mut Criterion) {
    c.bench_function("retry_delay_secs", |b| {
        b.iter(|| {
            // exercise a typical retry sequence
            for attempt in 0..10 {
                black_box(retry_delay_secs(attempt));
            }
        })
    });

    c.bench_function("should_bounce", |b| {
        b.iter(|| {
            for attempt in 0..10 {
                black_box(should_bounce(attempt, 5));
            }
        })
    });
}

criterion_group!(benches, bench_dkim_sign, bench_retry_policy);
criterion_main!(benches);
