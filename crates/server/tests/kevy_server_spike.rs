//! Comprehensive in-process kevy-server semantic spike (S0.5.1).
//!
//! The receiver-decoupling architecture inserts a shared **kevy-server**
//! layer between the stateless receiver and the stateful core. It carries
//! exactly two things: the anti-abuse state (greylist triplets + rate-limit
//! counters) and the pub/sub **notification** channel. The whole design
//! rests on that layer's command semantics being correct — if a greylist
//! TTL silently drops, senders get re-deferred forever; if a notification
//! is treated as durable, lost mail never gets indexed.
//!
//! The network client for kevy-server is a kevy dogfood deliverable that
//! has not shipped yet (and the dogfood rule forbids mailrs writing its
//! own). Until it lands, the in-process `kevy_embedded::Store` is the
//! stand-in — same engine, same command semantics, no network layer. These
//! tests pin those semantics, driven **through the real mailrs stone**
//! (`mailrs-shield`'s `GreylistDb`) where one exists and against the raw
//! `Store` primitives otherwise, so the eventual network cutover (P4/P6)
//! is a transport swap, not a semantic gamble.
//!
//! No database, no Docker — kevy is in-process. Runs on both backend axes.
//!
//! Not covered here (honest scope): **fail-open** (anti checks must let
//! mail through when kevy is unreachable) lives in the pipeline *stage*
//! that consumes these results, not in the kevy command layer — an
//! in-process store can't be made "unreachable" to exercise it cleanly, so
//! it belongs with the stage's own tests, not this foundation spike.

use std::time::{Duration, Instant};

use kevy_embedded::{Config, PubsubFrame, Store, Subscription};
use mailrs_shield::greylist::{GreylistConfig, GreylistDb, GreylistDecision, triplet_key};

fn mem_store() -> Store {
    Store::open(Config::default()).expect("open in-memory kevy")
}

/// Drain frames until a `Message` on `chan` arrives, panicking on timeout.
/// `subscribe` enqueues a `Subscribe` ack first, so callers must skip
/// non-`Message` frames — exactly what the production std::thread bridge
/// (outbound-queue worker) does.
fn recv_message(sub: &Subscription, chan: &[u8]) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match sub.recv_timeout(Duration::from_millis(100)) {
            Ok(PubsubFrame::Message { channel, payload }) if channel == chan => return payload,
            Ok(_) | Err(_) => continue,
        }
    }
    panic!("no Message frame on {chan:?} within deadline");
}

/// True if any `Message` on `chan` arrives within a short window.
fn got_message(sub: &Subscription, chan: &[u8]) -> bool {
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        match sub.recv_timeout(Duration::from_millis(50)) {
            Ok(PubsubFrame::Message { channel, .. }) if channel == chan => return true,
            Ok(_) | Err(_) => continue,
        }
    }
    false
}

#[tokio::test]
async fn greylist_triplet_lifecycle_via_real_stone() {
    // The receiver's actual greylist path — mailrs-shield GreylistDb on
    // in-process kevy, no PG. Driving `now` explicitly walks all three
    // decisions without sleeping.
    let store = mem_store();
    let db = GreylistDb::new(store.clone());
    let cfg = GreylistConfig::default(); // initial_delay 300s, pass_ttl 36d
    let key = triplet_key("203.0.113.5", "sender@ext.example", "alice@example.com");
    let t0 = 1_700_000_000u64;

    // first sight → defer, and record first_seen with the pass TTL
    assert_eq!(db.check(&key, t0, &cfg).await, GreylistDecision::Defer);

    // the triplet was stored with a TTL on the order of pass_ttl_secs
    let stored_ttl_ms = store.ttl_ms(format!("gl:{key}").as_bytes());
    let pass_ttl_ms = (cfg.pass_ttl_secs as i64) * 1000;
    assert!(
        stored_ttl_ms > pass_ttl_ms - 60_000 && stored_ttl_ms <= pass_ttl_ms,
        "triplet TTL ~= pass_ttl (was {stored_ttl_ms}ms, expected ~{pass_ttl_ms}ms)"
    );

    // retried inside the delay window → still defer
    assert_eq!(
        db.check(&key, t0 + 100, &cfg).await,
        GreylistDecision::TooEarly
    );
    // retried after the delay window → accept
    assert_eq!(
        db.check(&key, t0 + cfg.initial_delay_secs, &cfg).await,
        GreylistDecision::Accept
    );

    // a different triplet is independent — defers from scratch even though
    // the first one is already accepted
    let other = triplet_key("198.51.100.7", "sender@ext.example", "alice@example.com");
    assert_eq!(
        db.check(&other, t0 + cfg.initial_delay_secs, &cfg).await,
        GreylistDecision::Defer
    );
}

#[test]
fn rate_limit_token_bucket_via_incr_expire() {
    // The future kevy-server rate backend (S4.2): a per-key counter with a
    // sliding window expressed as INCR + EXPIRE. mailrs's rate-limit stone
    // is still in-process DashMap, so unlike greylist these semantics are
    // NOT yet exercised by production code — pin them now.
    let store = mem_store();
    let key = b"rate:203.0.113.5".as_slice();

    // first hit creates the counter + opens the window; hits inside the
    // window accumulate
    assert_eq!(store.incr(key).unwrap(), 1);
    store.expire(key, Duration::from_secs(1)).unwrap();
    assert_eq!(store.incr(key).unwrap(), 2);
    assert_eq!(store.incr(key).unwrap(), 3);
    assert!(store.ttl_ms(key) > 0, "window TTL is set");

    // after the window elapses the key is gone and the counter restarts —
    // proves kevy honors EXPIRE (the historical TTL-not-honored bug,
    // INC-2026-06-09, is fixed in this version)
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        store.exists(&[key]).unwrap(),
        0,
        "counter key expired after its window"
    );
    assert_eq!(
        store.incr(key).unwrap(),
        1,
        "rate counter restarts cleanly after the window"
    );
}

#[test]
fn pubsub_notification_delivery_and_nonpersistence() {
    // The notification channel: receiver PUBLISHes new-mail, core SUBSCRIBEs.
    let store = mem_store();
    let chan = b"notify:new-mail".as_slice();

    // (a) a live subscriber receives the publish, payload intact
    let sub = store.subscribe(&[chan]);
    let received = store.publish(chan, br#"{"id":42}"#);
    assert_eq!(received, 1, "the live subscriber received the publish");
    assert_eq!(recv_message(&sub, chan), br#"{"id":42}"#);

    // (b) non-persistence: a publish with no live subscriber reaches nobody
    // (returns 0) and is gone — a subscriber that joins afterwards never
    // sees it. This is precisely why notify is "fast but droppable" and the
    // core's durable guarantee comes from the maildir + reconcile backstop
    // (S2.2), not from pub/sub.
    let unwatched = b"notify:nobody-home".as_slice();
    let reached = store.publish(unwatched, br#"{"id":99}"#);
    assert_eq!(reached, 0, "publish with no subscriber reaches nobody");

    let late = store.subscribe(&[unwatched]);
    assert!(
        !got_message(&late, unwatched),
        "a late subscriber sees nothing of a past publish — pub/sub is not a queue"
    );
}

#[test]
fn greylist_first_seen_and_ttl_survive_restart() {
    // Greylist first-seen + its 36-day TTL must survive a server restart
    // (kevy AOF replay with absolute PEXPIREAT), or every sender gets
    // re-deferred on every redeploy. This is the durability half of the
    // kevy-server contract.
    let dir = tempfile::tempdir().unwrap();
    let key = b"gl:203.0.113.5|s@ext.example|alice@example.com".as_slice();
    let ttl = Duration::from_secs(36 * 24 * 3600);

    {
        let store = Store::open(Config::default().with_persist(dir.path())).unwrap();
        store.set_with_ttl(key, b"1700000000", ttl).unwrap();
        assert!(store.ttl_ms(key) > 0);
        // NB: kevy's Store::flush() is Redis FLUSHALL (wipe), not an fsync —
        // do NOT call it here. set_with_ttl already appended to the AOF; the
        // shard's BufWriter lands on disk when the store drops at the end of
        // this scope, which is the "restart" we then reopen from.
    }

    // reopen the same data dir == a server restart
    let store = Store::open(Config::default().with_persist(dir.path())).unwrap();
    assert_eq!(
        store.get(key).unwrap().as_deref(),
        Some(&b"1700000000"[..]),
        "first_seen value survives restart"
    );
    let ttl_after = store.ttl_ms(key);
    assert!(
        ttl_after > 0,
        "greylist TTL survives restart (was {ttl_after}ms)"
    );
    assert!(
        ttl_after as u128 <= ttl.as_millis(),
        "TTL is not inflated on reload (was {ttl_after}ms)"
    );
}
