//! Own path first, external channels for alerts about the path itself and
//! for what the sender never confirms. Real HTTP receivers stand in for the
//! core's alert route and the webhook; the clock is the `now` passed in.

use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode};

use crate::config::Config;
use crate::engine::Notice;
use crate::engine::machine::NoticeKind;
use crate::notify::Dispatcher;
use crate::rules;
use crate::store::Store;

type Seen = Arc<Mutex<Vec<(Option<String>, serde_json::Value)>>>;

/// An HTTP endpoint that keeps each body and its bearer.
async fn endpoint(path: &'static str) -> (String, Seen) {
    let seen: Seen = Arc::default();
    let keep = seen.clone();
    let app = axum::Router::new().route(
        path,
        axum::routing::post(move |headers: HeaderMap, body: String| {
            let keep = keep.clone();
            async move {
                let bearer = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                keep.lock()
                    .unwrap()
                    .push((bearer, serde_json::from_str(&body).unwrap_or_default()));
                StatusCode::OK
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await });
    (format!("http://{addr}"), seen)
}

fn notice(name: &str, object: &str, at: i64) -> Notice {
    let r = rules::all().into_iter().find(|r| r.name == name).unwrap();
    Notice {
        rule: r,
        object: object.into(),
        kind: NoticeKind::Firing,
        severity: r.severity,
        value: "v".into(),
        threshold: "t".into(),
        since: at,
        at,
        under: None,
    }
}

const T: i64 = 1_800_000_000 / 3600 * 3600;

struct Rig {
    d: Dispatcher,
    core: Seen,
    hook: Seen,
}

async fn rig(external: bool) -> Rig {
    let (core_url, core) = endpoint(mailrs_core_api::method::ops_alert::PATH_OPS_ALERTS).await;
    let (hook_url, hook) = endpoint("/hook").await;
    let mut c = Config::for_tests(std::env::temp_dir());
    c.own_path = true;
    c.fastcore_url = core_url;
    if external {
        c.webhook_url = Some(format!("{hook_url}/hook"));
    }
    Rig {
        d: Dispatcher::new(&c, Arc::new(Store::in_memory().unwrap())),
        core,
        hook,
    }
}

async fn fire(r: &mut Rig, n: Notice, at: i64) {
    r.d.accept(&[n], &[], false, at);
    r.d.tick(at + 30).await;
}

#[tokio::test]
async fn an_ordinary_alert_takes_the_own_path_as_the_daemon_key() {
    let mut r = rig(true).await;
    fire(&mut r, notice("spool_quarantined", "spool", T), T).await;
    let core = r.core.lock().unwrap().clone();
    assert_eq!(core.len(), 1);
    let key = mailrs_ops_report::key::derive(b"test-master", "ops").unwrap();
    assert_eq!(core[0].0.as_deref(), Some(format!("Bearer {key}").as_str()));
    let fields: Vec<&String> = core[0].1.as_object().unwrap().keys().collect();
    assert_eq!(
        fields,
        ["id", "subject", "text"],
        "no recipients in the request"
    );
    assert!(
        r.hook.lock().unwrap().is_empty(),
        "not sent outside as well"
    );
}

#[tokio::test]
async fn an_alert_about_the_sending_path_goes_straight_outside() {
    for (rule, object) in [
        ("outbound_stuck", "sender"),
        (rules::KEVY_SERVER_UNAVAILABLE, "kevy-server"),
        ("source_unreadable.fastcore", "fastcore"),
        ("spool_stalled", "spool"),
        ("crash_loop", "sender/abc"),
    ] {
        let mut r = rig(true).await;
        fire(&mut r, notice(rule, object, T), T).await;
        assert!(
            r.core.lock().unwrap().is_empty(),
            "{rule} tried the own path"
        );
        assert_eq!(r.hook.lock().unwrap().len(), 1, "{rule}");
    }
    // a crash loop of a process off the path is an ordinary alert
    let mut r = rig(true).await;
    fire(&mut r, notice("crash_loop", "backup/abc", T), T).await;
    assert_eq!(r.core.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_queued_answer_is_not_delivery_and_the_fallback_comes_after_five_minutes() {
    let mut r = rig(true).await;
    fire(&mut r, notice("spool_quarantined", "spool", T), T).await;
    assert_eq!(r.core.lock().unwrap().len(), 1, "the core answered 200");
    r.d.tick(T + 30 + 299).await;
    assert!(r.hook.lock().unwrap().is_empty(), "inside the window");
    r.d.tick(T + 30 + 300).await;
    let hook = r.hook.lock().unwrap().clone();
    assert_eq!(
        hook.len(),
        1,
        "no delivered marker: the external channel gets it"
    );
    assert_eq!(hook[0].1["alerts"][0]["rule"], "spool_quarantined");
}

#[tokio::test]
async fn the_senders_marker_ends_the_wait() {
    let mut r = rig(true).await;
    fire(&mut r, notice("spool_quarantined", "spool", T), T).await;
    let id = r.core.lock().unwrap()[0].1["id"]
        .as_str()
        .unwrap()
        .to_string();
    r.d.mark_delivered(&["unrelated".into()], T + 60);
    assert_eq!(r.d.pending_ids(), [id.clone()], "another id does not count");
    r.d.mark_delivered(&[id], T + 120);
    r.d.tick(T + 3600).await;
    assert!(r.hook.lock().unwrap().is_empty());
    assert!(r.d.pending_ids().is_empty());
}

#[tokio::test]
async fn without_external_channels_the_own_path_is_all_there_is() {
    let mut r = rig(false).await;
    assert!(!r.d.has_external());
    fire(&mut r, notice("outbound_stuck", "sender", T), T).await;
    assert_eq!(
        r.core.lock().unwrap().len(),
        1,
        "tried anyway, and logged so"
    );
    r.d.tick(T + 3600).await;
    assert_eq!(
        r.core.lock().unwrap().len(),
        1,
        "no second attempt on the same path"
    );
}
