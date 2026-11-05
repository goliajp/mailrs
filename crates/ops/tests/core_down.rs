//! A core that stops answering: the daemon keeps running, sends one grouped
//! notice by webhook and by the external relay (mailpit here), nothing
//! enters the local outbound queue, and a recovery notice follows when the
//! core is back.

mod common;

use std::sync::Arc;
use std::time::Duration;

use mailrs_ops::app::App;
use mailrs_ops::clock::Clock;
use mailrs_ops::config::{Config, RelayConfig};
use mailrs_ops::store::Store;

const SIDE_STATE: &str = "mem://ops-core-down-side-state";

fn fastcore_state(maildir: &std::path::Path) -> Arc<mailrs_fastcore::FastcoreState> {
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let mut fc = mailrs_fastcore::FastcoreState::new(mailbox);
    fc.net_url = Some(SIDE_STATE.to_string());
    fc.maildir_root = maildir.to_string_lossy().into_owned();
    Arc::new(fc)
}

/// The core on its own runtime, so stopping it ends its open connections
/// too, as a stopped process does; the returned sender stops it.
fn serve_core(
    state: Arc<mailrs_fastcore::FastcoreState>,
    addr: std::net::SocketAddr,
) -> std::sync::mpsc::Sender<()> {
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let (up, is_up) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.spawn(async move {
            let l = tokio::net::TcpListener::bind(addr)
                .await
                .expect("bind the core's port");
            let router = mailrs_fastcore::build_router_with_secret(state, String::new());
            let _ = up.send(());
            let _ = axum::serve(l, router).await;
        });
        let _ = stopped.recv();
        rt.shutdown_timeout(Duration::from_secs(1));
    });
    is_up
        .recv_timeout(Duration::from_secs(10))
        .expect("core up");
    stop
}

/// Cycle once a second for `secs`, flushing between.
async fn run(app: &Arc<App>, secs: u64) {
    for _ in 0..secs {
        app.cycle().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        app.flush().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_core_is_one_grouped_alert_by_webhook_and_relay_and_never_the_local_queue() {
    let side_state = kevy_client::Connection::connect(SIDE_STATE).unwrap();
    let maildir = tempfile::tempdir().unwrap();
    let state = fastcore_state(maildir.path());
    let addr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap()
    };
    let core = serve_core(state.clone(), addr);
    let (hook, hook_inbox) = common::receiver().await;
    let (beat, beats) = common::receiver().await;
    let mailpit = common::mailpit().await;
    let data = tempfile::tempdir().unwrap();

    let mut cfg = Config::for_tests(data.path().to_path_buf());
    cfg.fastcore_url = format!("http://{addr}");
    cfg.webapi_url = common::webapi(true).await;
    cfg.webhook_url = Some(hook);
    cfg.heartbeat_url = Some(beat);
    cfg.relay = Some(RelayConfig {
        host: mailpit.smtp_host.clone(),
        port: mailpit.smtp_port,
        starttls: false,
        user: None,
        password: None,
        from: "ops@outside.invalid".into(),
        to: vec!["oncall@outside.invalid".into()],
    });
    cfg.interval_secs = 1;
    cfg.group_secs = 2;
    cfg.unknown_wait_secs = 3;
    cfg.push_sources = Vec::new();
    let app = App::open(
        cfg,
        Clock::System,
        Store::open(&data.path().join("ops.sqlite")).unwrap(),
    )
    .unwrap();

    // whether this host gives the core a cgroup to read is not the test's
    // business; its "cannot read the core's memory" is silenced
    app.store
        .set_silence(&mailrs_ops::store::Silence {
            id: "host-cgroup".into(),
            rule: "core_memory_high".into(),
            object: None,
            until: mailrs_ops_report::build::now() + 3600,
            reason: "depends on the test host".into(),
            by: "test".into(),
            set_at: mailrs_ops_report::build::now(),
        })
        .unwrap();
    run(&app, 4).await;
    assert!(
        common::alerts(&hook_inbox).is_empty(),
        "healthy: {:?}",
        hook_inbox.lock().unwrap()
    );
    let healthy_beats = beats.lock().unwrap().len();
    assert!(
        healthy_beats >= 4,
        "a heartbeat after each completed cycle: {healthy_beats}"
    );

    core.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for _ in 0..15 {
        run(&app, 1).await;
        if !common::alerts(&hook_inbox).is_empty() {
            break;
        }
    }
    run(&app, 3).await;
    let sent = common::alerts(&hook_inbox);
    assert_eq!(sent.len(), 1, "one grouped message: {sent:?}");
    assert_eq!(
        common::rules_in(&sent[0]),
        [(
            "source_unreadable.fastcore".to_string(),
            "firing".to_string()
        )],
        "the core's own rules are unknown, not separate notices"
    );
    let mut mails = Vec::new();
    assert!(
        common::until(Duration::from_secs(10), || {
            mails = subjects_now(&mailpit);
            mails.len() == 1
        })
        .await,
        "relay: {mails:?}"
    );

    let pending = mailrs_core_sidestate::families::outbound::PENDING_IDX;
    let mut c = kevy_client::Connection::connect(SIDE_STATE).unwrap();
    assert_eq!(
        c.llen(pending).unwrap(),
        0,
        "nothing went through the local outbound queue"
    );
    drop(side_state);

    let _core = serve_core(state, addr);
    for _ in 0..10 {
        run(&app, 1).await;
        if common::alerts(&hook_inbox).len() > 1 {
            break;
        }
    }
    let sent = common::alerts(&hook_inbox);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(
        common::rules_in(&sent[1]),
        [(
            "source_unreadable.fastcore".to_string(),
            "resolved".to_string()
        )]
    );
    assert!(
        common::until(Duration::from_secs(10), || {
            mails = subjects_now(&mailpit);
            mails.len() == 2
        })
        .await,
        "relay: {mails:?}"
    );
}

/// The subjects of the alert mails mailpit holds (the channel test the
/// daemon sends on its first cycle left out), read from a blocking context
/// inside `until`.
fn subjects_now(m: &common::Mailpit) -> Vec<String> {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(m.subjects()))
        .into_iter()
        .filter(|s| !s.contains("test message"))
        .collect()
}
