use mailrs_receiver::smtp_session::probe_guard::{ACCOUNT_LIMIT, LIMIT};

use crate::harness::{Client, REAL, SPF_DOMAIN, receiver};

/// `n` unknown local recipients, each refused as unknown.
async fn miss(c: &mut Client, n: u32, tag: &str) {
    for i in 0..n {
        let r = c.say(&format!("RCPT TO:<{tag}.{i}@golia.test>")).await;
        assert!(r.starts_with("550 5.1.1"), "miss {i}: {r}");
    }
}

async fn assert_cut_off(c: &mut Client) {
    let r = c.say(&format!("RCPT TO:<{REAL}>")).await;
    assert!(
        r.starts_with("421 4.7.0"),
        "once cut off, an existing address gets the same answer: {r}"
    );
    assert_eq!(c.reply().await, "", "the connection is closed");
}

#[tokio::test]
async fn an_ip_is_cut_off_after_ten_misses() {
    let rx = receiver("probe-ip", None).await;
    let mut c = Client::connect(rx.port).await;
    c.say("MAIL FROM:<x@nospf.example>").await;
    let r = c.say(&format!("RCPT TO:<{REAL}>")).await;
    assert!(r.starts_with("250"), "an existing address is accepted: {r}");
    miss(&mut c, LIMIT, "guess").await;
    assert_eq!(rx.counts.tripped(), 0, "nothing refused yet");
    assert_cut_off(&mut c).await;
    assert_eq!(rx.counts.tripped(), 1);

    let mut again = Client::connect(rx.port).await;
    again.say("MAIL FROM:<x@nospf.example>").await;
    let r = again.say("RCPT TO:<someone@remote.example>").await;
    assert!(
        !r.is_empty() && !r.starts_with("421"),
        "a remote recipient is not guarded: {r}"
    );
    let r = again.say("RCPT TO:<guess.x@golia.test>").await;
    assert!(
        r.starts_with("421"),
        "a new connection from the same IP: {r}"
    );
    assert_eq!(rx.counts.tripped(), 2);
}

#[tokio::test]
async fn an_spf_passing_sender_is_counted_apart_from_its_ip() {
    let rx = receiver("probe-spf", None).await;
    let mut c = Client::connect(rx.port).await;
    c.say(&format!("MAIL FROM:<x@{SPF_DOMAIN}>")).await;
    miss(&mut c, LIMIT, "spf").await;
    assert_cut_off(&mut c).await;

    // the same IP with a domain whose SPF does not pass has its own count
    let mut other = Client::connect(rx.port).await;
    other.say("MAIL FROM:<x@nospf.example>").await;
    miss(&mut other, LIMIT, "ip").await;
    assert_cut_off(&mut other).await;

    // and the SPF sender is still cut off
    let mut back = Client::connect(rx.port).await;
    back.say(&format!("MAIL FROM:<y@{SPF_DOMAIN}>")).await;
    let r = back.say("RCPT TO:<guess.z@golia.test>").await;
    assert!(r.starts_with("421"), "{r}");
}

#[tokio::test]
async fn a_signed_in_account_is_cut_off_after_thirty_misses() {
    let rx = receiver("probe-account", None).await;
    let mut c = Client::signed_in(rx.port).await;
    c.say("MAIL FROM:<me@golia.test>").await;
    miss(&mut c, ACCOUNT_LIMIT, "acct").await;
    assert_cut_off(&mut c).await;
    assert_eq!(rx.counts.tripped(), 1);

    // the account, not the IP: an unauthenticated sender from the same IP
    // still has its own ten
    let mut anon = Client::connect(rx.port).await;
    anon.say("MAIL FROM:<x@nospf.example>").await;
    miss(&mut anon, 1, "anon").await;
}
