//! The dispatcher against real HTTP receivers on loopback.

use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use super::*;
use crate::config::{Config, RelayConfig};
use crate::engine::Notice;
use crate::engine::machine::NoticeKind;
use crate::rules::{self, Severity};

#[derive(Default)]
struct Inbox {
    bodies: Vec<(HeaderMap, String)>,
    /// Answer 500 to this many requests first.
    fail_first: usize,
    seen: usize,
}

type Shared = Arc<Mutex<Inbox>>;

async fn take(State(i): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let mut i = i.lock().unwrap();
    i.seen += 1;
    if i.seen <= i.fail_first {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    i.bodies
        .push((headers, String::from_utf8_lossy(&body).into_owned()));
    StatusCode::OK
}

async fn receiver(fail_first: usize) -> (String, Shared) {
    let shared: Shared = Arc::new(Mutex::new(Inbox {
        fail_first,
        ..Default::default()
    }));
    let app = axum::Router::new()
        .route("/hook", axum::routing::post(take))
        .with_state(shared.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await });
    (format!("http://{addr}/hook"), shared)
}

fn rule(name: &str) -> &'static crate::rules::Rule {
    rules::all().into_iter().find(|r| r.name == name).unwrap()
}

fn notice(name: &str, object: &str, value: &str, at: i64) -> Notice {
    let r = rule(name);
    Notice {
        rule: r,
        object: object.into(),
        kind: NoticeKind::Firing,
        severity: r.severity,
        value: value.into(),
        threshold: "x".into(),
        since: at,
        at,
        under: None,
    }
}

fn cfg() -> Config {
    let mut c = Config::for_tests(std::env::temp_dir());
    c.webhook_secret = Some("hook-secret".into());
    c
}

fn dispatcher(c: &Config) -> Dispatcher {
    Dispatcher::new(c, Arc::new(Store::in_memory().unwrap()))
}

const T: i64 = 1_800_000_000 / 3600 * 3600;

#[tokio::test]
async fn notices_within_the_window_go_out_as_one_signed_message() {
    let (url, inbox) = receiver(0).await;
    let mut c = cfg();
    c.webhook_url = Some(url);
    let mut d = dispatcher(&c);
    d.accept(
        &[notice("spool_quarantined", "spool", "1", T)],
        &[],
        false,
        T,
    );
    d.accept(
        &[notice("crash_loop", "receiver/a", "3 starts", T + 10)],
        &[],
        false,
        T + 10,
    );
    d.tick(T + 29).await;
    assert!(inbox.lock().unwrap().bodies.is_empty(), "inside the window");
    d.tick(T + 30).await;
    let got = inbox.lock().unwrap();
    assert_eq!(got.bodies.len(), 1);
    let (headers, body) = &got.bodies[0];
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["alerts"].as_array().unwrap().len(), 2);
    assert!(
        !v["alerts"][1]["first_step"]["zh"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    let sig = headers["x-mailrs-signature"].to_str().unwrap();
    assert!(mailrs_webhook_signature::verify_header(
        b"hook-secret",
        body.as_bytes(),
        sig
    ));
}

#[tokio::test]
async fn the_eleventh_message_in_an_hour_waits_for_the_digest() {
    let (url, inbox) = receiver(0).await;
    let mut c = cfg();
    c.webhook_url = Some(url);
    let mut d = dispatcher(&c);
    for i in 0..11 {
        let at = T + i * 60;
        d.accept(
            &[notice("spool_quarantined", "spool", "1", at)],
            &[],
            false,
            at,
        );
        d.tick(at + 30).await;
    }
    assert_eq!(inbox.lock().unwrap().bodies.len(), 10);
    // a test message is outside the budget
    d.test(T + 1000).await;
    assert_eq!(inbox.lock().unwrap().bodies.len(), 11);
    d.tick(T + 3600).await;
    let got = inbox.lock().unwrap();
    assert_eq!(got.bodies.len(), 12);
    let digest: serde_json::Value = serde_json::from_str(&got.bodies[11].1).unwrap();
    assert_eq!(digest["kind"], "digest");
    assert!(
        digest["summary"].as_str().unwrap().starts_with("1 more"),
        "{digest}"
    );
}

#[tokio::test]
async fn maintenance_holds_notices_except_for_data_in_danger_and_sums_up_at_the_end() {
    let (url, inbox) = receiver(0).await;
    let mut c = cfg();
    c.webhook_url = Some(url);
    let mut d = dispatcher(&c);
    d.accept(
        &[notice("spool_quarantined", "spool", "1", T)],
        &[],
        true,
        T,
    );
    d.accept(
        &[notice("data_volume_low", "mailrs-data", "1 GB free", T)],
        &[],
        true,
        T,
    );
    d.accept(
        &[notice("maintenance_too_long", "maintenance", "open 5h", T)],
        &[],
        true,
        T,
    );
    d.tick(T + 30).await;
    {
        let got = inbox.lock().unwrap();
        assert_eq!(got.bodies.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&got.bodies[0].1).unwrap();
        let rules: Vec<_> = v["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["rule"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(rules, ["data_volume_low", "maintenance_too_long"]);
    }
    d.maintenance_ended(T, T + 100).await;
    let got = inbox.lock().unwrap();
    let v: serde_json::Value = serde_json::from_str(&got.bodies[1].1).unwrap();
    assert_eq!(v["kind"], "maintenance_ended");
    assert_eq!(v["alerts"][0]["rule"], "spool_quarantined");
}

#[tokio::test]
async fn a_silenced_or_filed_notice_is_not_sent() {
    let (url, inbox) = receiver(0).await;
    let mut c = cfg();
    c.webhook_url = Some(url);
    let mut d = dispatcher(&c);
    let s = Silence {
        id: "s1".into(),
        rule: "spool_quarantined".into(),
        object: None,
        until: T + 3600,
        reason: "known".into(),
        by: "cli:a".into(),
        set_at: T,
    };
    let mut filed = notice("kevy_server_memory_high", "kevy-server", "x", T);
    filed.under = Some(rules::KEVY_SERVER_UNAVAILABLE);
    d.accept(
        &[notice("spool_quarantined", "spool", "1", T), filed],
        &[s],
        false,
        T,
    );
    d.tick(T + 60).await;
    assert!(inbox.lock().unwrap().bodies.is_empty());
}

#[tokio::test]
async fn a_minimal_push_names_no_address_or_local_domain() {
    let leaky = "open 5h by webapi:admin@example.test (moving mail.example.test)";
    for minimal in [true, false] {
        let (url, inbox) = receiver(0).await;
        let mut c = cfg();
        c.push_url = Some(url);
        c.push_minimal = minimal;
        c.admin_url = Some("https://ops-console.invalid/admin".into());
        let mut d = dispatcher(&c);
        d.accept(
            &[notice("maintenance_too_long", "maintenance", leaky, T)],
            &[],
            false,
            T,
        );
        d.tick(T + 30).await;
        let got = inbox.lock().unwrap();
        let (headers, body) = &got.bodies[0];
        let all = format!("{} {body}", headers["title"].to_str().unwrap());
        let leaks = all.contains('@') || c.local_domains.iter().any(|d| all.contains(d.as_str()));
        assert_eq!(leaks, !minimal, "minimal={minimal}: {all}");
        assert!(all.contains("maintenance_too_long"), "{all}");
        assert_eq!(
            all.contains("ops-console.invalid"),
            !minimal,
            "the link: {all}"
        );
    }
}

#[test]
fn the_relay_refuses_a_recipient_in_a_local_domain() {
    let mut r = RelayConfig {
        host: "relay.invalid".into(),
        port: 587,
        starttls: true,
        user: None,
        password: None,
        from: "ops@outside.invalid".into(),
        to: vec!["oncall@outside.invalid".into()],
    };
    let local = ["example.test".to_string()];
    assert_eq!(relay_refusal(&r, &local), None);
    r.to.push("postmaster@Example.Test".into());
    assert!(relay_refusal(&r, &local).is_some());
    let mut c = cfg();
    c.relay = Some(r);
    let d = dispatcher(&c);
    assert_eq!(d.status()["channels"][0]["enabled"], false);
    assert!(
        d.channel_views().is_empty(),
        "a refused channel is not used"
    );
}

#[tokio::test]
async fn a_failing_channel_is_reported_through_the_others() {
    let (hook, hook_inbox) = receiver(usize::MAX).await;
    let (push, push_inbox) = receiver(0).await;
    let mut c = cfg();
    c.webhook_url = Some(hook);
    c.push_url = Some(push);
    let mut d = dispatcher(&c);
    for i in 0..3 {
        let at = T + i * 60;
        d.accept(
            &[notice("spool_quarantined", "spool", "1", at)],
            &[],
            false,
            at,
        );
        d.tick(at + 30).await;
    }
    let views = d.channel_views();
    assert_eq!(
        views
            .iter()
            .find(|v| v.name == "webhook")
            .unwrap()
            .consecutive_failures,
        3
    );
    let seen_before = hook_inbox.lock().unwrap().seen;
    let mut n = notice(
        rules::CHANNEL_FAILING,
        "webhook",
        "3 failures in a row",
        T + 300,
    );
    n.severity = Severity::Warning;
    d.accept(&[n], &[], false, T + 300);
    d.tick(T + 330).await;
    assert_eq!(
        hook_inbox.lock().unwrap().seen,
        seen_before,
        "not tried on itself"
    );
    let pushed = push_inbox.lock().unwrap();
    assert!(pushed.bodies.last().unwrap().1.contains("channel_failing"));
}

#[test]
fn the_relay_mail_carries_both_languages_in_base64() {
    use base64::Engine as _;
    let r = RelayConfig {
        host: "h".into(),
        port: 25,
        starttls: false,
        user: None,
        password: None,
        from: "ops@outside.invalid".into(),
        to: vec!["a@outside.invalid".into()],
    };
    let msg = Message {
        id: "1-1".into(),
        at: T,
        host: "mx".into(),
        kind: MessageKind::Alerts,
        notices: vec![Sent::from(&notice("spool_quarantined", "spool", "1", T))],
        link: None,
    };
    let raw = String::from_utf8(mail_bytes(&r, &msg)).unwrap();
    let body = raw.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
    let text = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap(),
    )
    .unwrap();
    assert!(
        text.contains("Mail was quarantined") && text.contains("有信被隔离"),
        "{text}"
    );
}
