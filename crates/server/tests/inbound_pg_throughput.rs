//! E2E inbound throughput baseline (PG-backed).
//!
//! Calls `PgMailboxStore::append_message` directly — the same entry the SMTP
//! receive pipeline uses after auth/policy stages — and measures sustained
//! delivery throughput + per-call latency percentiles + sqlx pool stats.
//!
//! Excluded by design: SPF/DKIM/DMARC verify (no DNS), greylist/DNSBL
//! shield, sieve, Valkey cache. Those have their own micro-benches; this
//! harness measures the PG-anchored persistence cost the user observes
//! once a message has been accepted.
//!
//! Requires a running PostgreSQL via `MAILRS_PG_URL` (typically
//! `docker compose up postgres -d`). All tests `#[ignore]` so default
//! `cargo test --workspace` skips them; opt in with:
//!
//!   MAILRS_PG_URL=postgres://mailrs:mailrs@127.0.0.1/mailrs \
//!     cargo test -p mailrs-server --test inbound_pg_throughput \
//!     -- --ignored --nocapture
//!
//! Knobs (env vars, all optional):
//!   * BASELINE_MSGS              total messages to deliver (default 1_000)
//!   * BASELINE_WORKERS           concurrent worker tasks (default 8)
//!   * BASELINE_POOL_MAX          PG pool max_connections (default 20)
//!   * BASELINE_TEST_BEFORE_ACQUIRE   `0` or `1` (default 1, matches prod)
//!   * BASELINE_MAILBOX_FANOUT    1 = single hot mailbox (FOR UPDATE serial),
//!     N = round-robin across N mailboxes (default 1)
//!
//! Output: a single `BASELINE_RESULT=` line so callers can `grep` it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mailrs_mailbox::PgMailboxStore;
use sqlx::postgres::PgPoolOptions;

const SAMPLE_MESSAGE: &[u8] = b"From: sender@example.com\r\n\
To: alice@example.com\r\n\
Subject: baseline\r\n\
Message-ID: <{N}@example.com>\r\n\
\r\n\
hello world\r\n";

fn env<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

async fn ensure_test_user(pool: &sqlx::PgPool, user: &str, n_mailboxes: u32) {
    // domain + account rows
    let (local, domain) = user.split_once('@').expect("user format");
    sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(domain)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (address, domain, password_hash) VALUES ($1, $2, 'unused')
         ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .bind(domain)
    .execute(pool)
    .await
    .unwrap();
    // per-fanout mailboxes
    for i in 0..n_mailboxes {
        let name = if n_mailboxes == 1 {
            "INBOX".to_string()
        } else {
            format!("INBOX{i}")
        };
        sqlx::query(
            "INSERT INTO mailboxes (user_address, name, uidvalidity, uidnext, highest_modseq)
             VALUES ($1, $2, 1, 1, 0) ON CONFLICT DO NOTHING",
        )
        .bind(user)
        .bind(&name)
        .execute(pool)
        .await
        .unwrap();
    }
    // cleanup prior messages so successive runs start from a known state
    sqlx::query(
        "DELETE FROM messages WHERE mailbox_id IN (SELECT id FROM mailboxes WHERE user_address = $1)",
    )
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE mailboxes SET uidnext = 1, highest_modseq = 0 WHERE user_address = $1",
    )
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    let _ = local;
}

fn pct(latencies_us: &[u64], p: f64) -> u64 {
    if latencies_us.is_empty() {
        return 0;
    }
    let i = ((latencies_us.len() as f64) * p).floor() as usize;
    latencies_us[i.min(latencies_us.len() - 1)]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn baseline_inbound_delivery() {
    let url = std::env::var("MAILRS_PG_URL").expect("MAILRS_PG_URL required");
    let msgs: usize = env("BASELINE_MSGS", 1_000);
    let workers: usize = env("BASELINE_WORKERS", 8);
    let pool_max: u32 = env("BASELINE_POOL_MAX", 20);
    let test_before_acquire: u8 = env("BASELINE_TEST_BEFORE_ACQUIRE", 1);
    let fanout: u32 = env("BASELINE_MAILBOX_FANOUT", 1);

    let pool = PgPoolOptions::new()
        .max_connections(pool_max)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .test_before_acquire(test_before_acquire != 0)
        .connect(&url)
        .await
        .expect("PG pool");

    let user = "alice@example.com";
    ensure_test_user(&pool, user, fanout).await;

    let maildir_root = std::env::temp_dir()
        .join(format!("mailrs-baseline-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let _ = std::fs::remove_dir_all(&maildir_root);

    let store = Arc::new(PgMailboxStore::new(pool.clone()));
    let counter = Arc::new(AtomicUsize::new(0));
    let now_unix = chrono::Utc::now().timestamp();

    let mut handles = Vec::with_capacity(workers);
    let started = Instant::now();
    for w in 0..workers {
        let store = store.clone();
        let counter = counter.clone();
        let maildir_root = maildir_root.clone();
        handles.push(tokio::spawn(async move {
            let mut latencies = Vec::with_capacity(msgs / workers + 1);
            loop {
                let n = counter.fetch_add(1, Ordering::Relaxed);
                if n >= msgs {
                    break;
                }
                let mb = if fanout == 1 {
                    "INBOX".to_string()
                } else {
                    format!("INBOX{}", n as u32 % fanout)
                };
                let body = SAMPLE_MESSAGE.to_vec();
                // cheap template substitution: replace `{N}` with the index
                let body = String::from_utf8(body).unwrap().replace("{N}", &n.to_string());

                let t0 = Instant::now();
                let r = store
                    .append_message(user, &mb, &maildir_root, body.as_bytes(), 0, now_unix)
                    .await;
                let dt_us = t0.elapsed().as_micros() as u64;
                if r.is_ok() {
                    latencies.push(dt_us);
                }
                let _ = w;
            }
            latencies
        }));
    }

    let mut all_latencies: Vec<u64> = Vec::with_capacity(msgs);
    for h in handles {
        all_latencies.extend(h.await.unwrap());
    }
    let elapsed = started.elapsed();
    all_latencies.sort_unstable();

    let throughput = all_latencies.len() as f64 / elapsed.as_secs_f64();
    let p50 = pct(&all_latencies, 0.50);
    let p95 = pct(&all_latencies, 0.95);
    let p99 = pct(&all_latencies, 0.99);
    let p999 = pct(&all_latencies, 0.999);

    let pool_size = pool.size();
    let pool_idle = pool.num_idle();

    println!(
        "BASELINE_RESULT msgs={} workers={} pool_max={} test_before_acquire={} fanout={} elapsed_ms={} throughput_msg_s={:.1} p50_us={} p95_us={} p99_us={} p999_us={} pool_size={} pool_idle={}",
        all_latencies.len(),
        workers,
        pool_max,
        test_before_acquire,
        fanout,
        elapsed.as_millis(),
        throughput,
        p50,
        p95,
        p99,
        p999,
        pool_size,
        pool_idle
    );
}
