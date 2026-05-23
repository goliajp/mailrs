//! Server-level perf regression gates.
//!
//! mailrs-server is a binary crate, so we can't `use` its internal
//! modules from a `tests/` file. What we *can* do is measure the
//! composition of stones the server links together — the same hot-path
//! recipe an inbound SMTP delivery follows: `smtp-proto` session +
//! command parse → `maildir` delivery. If a workspace bump silently
//! makes any of those slower in our actual call shape, this gate
//! catches it before deploy.
//!
//! Each test asserts a single budget with ~5-10× headroom over the
//! observed mean on a clean M-series Mac. The budgets are intentionally
//! loose: this file's job is to catch **order-of-magnitude regressions**
//! (a dep upgrade that ships 5× slower without our noticing), not
//! micro-perf swings. Per-stone micro-perf is covered by each crate's
//! own `tests/perf_gate.rs` + `benches/`.

use std::time::{Duration, Instant};

use mailrs_dmarc::{
    eval::{evaluate, DkimSignatureResult, DmarcInput, SpfResult},
    policy::DmarcPolicy,
};
use mailrs_maildir::Maildir;
use mailrs_rfc5322::Message;
use mailrs_smtp_proto::session::{Event, Session, SessionConfig};
use mailrs_smtp_proto::{parse_command, Command};

const HOSTNAME: &str = "mx.test.local";

/// Helper: run an SMTP envelope through the session machine in the shape
/// our real server does. Returns `true` when the session reached the
/// `NeedData` state. Doesn't actually deliver — that's the next test.
fn run_smtp_envelope(session: &mut Session) -> bool {
    for line in [
        "EHLO client.example",
        "MAIL FROM:<sender@example.com>",
        "RCPT TO:<recipient@example.com>",
        "DATA",
    ] {
        let cmd = parse_command(line).expect("parse_command");
        let event = session.handle_command(&cmd);
        if matches!(event, Event::NeedData { .. }) {
            return true;
        }
    }
    false
}

#[test]
fn smtp_envelope_dispatch_under_budget() {
    // Budget: per-envelope-only-no-DATA wall time, mean of 10k iterations.
    // On a clean M-series Mac the loop runs at sub-µs; budget at 50 µs
    // leaves ~50× headroom for slower CI environments.
    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let mut session = Session::new(HOSTNAME, SessionConfig::default());
        let reached_data = run_smtp_envelope(&mut session);
        std::hint::black_box(reached_data);
    }
    let elapsed = start.elapsed();
    let per_envelope = elapsed / iterations;
    assert!(
        per_envelope < Duration::from_micros(50),
        "SMTP envelope dispatch took {per_envelope:?} per message (budget: 50 µs)"
    );
}

#[test]
fn smtp_session_data_then_maildir_deliver_under_budget() {
    // Budget: full envelope + DATA + maildir deliver, mean of 1k messages.
    // The maildir write dominates and is filesystem-bound; budget at 5 ms
    // per message comfortably covers slow CI disks.
    let tmpdir = tempfile::tempdir().expect("tempdir");
    let maildir =
        Maildir::create(tmpdir.path().to_str().expect("path utf8")).expect("Maildir::create");

    let body = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: perf test\r\n\
                 Date: Mon, 23 May 2026 12:00:00 +0000\r\nMessage-ID: <abc@test>\r\n\r\n\
                 body bytes";

    let iterations = 1_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let mut session = Session::new(HOSTNAME, SessionConfig::default());
        let reached = run_smtp_envelope(&mut session);
        assert!(reached, "session did not reach NeedData");
        maildir.deliver(body).expect("maildir.deliver");
    }
    let elapsed = start.elapsed();
    let per_msg = elapsed / iterations;
    // Budget: 20 ms covers slow CI disks + macOS APFS sync overhead.
    // Tighten only if the workload becomes filesystem-bottlenecked enough
    // that we want to track it explicitly.
    assert!(
        per_msg < Duration::from_millis(20),
        "full SMTP→maildir per-msg {per_msg:?} (budget: 20 ms)"
    );
}

#[test]
fn rfc5322_lookup_chain_under_budget() {
    // Budget: lazy header lookup the server performs on every inbound
    // message (Subject + From + Received chain probe + body offset).
    // On a clean M-series Mac the lookup is < 100 ns; budget at 5 µs
    // leaves order-of-magnitude headroom.
    let raw = b"Received: from a.example by b.example with ESMTP id 1\r\n\
                Received: from b.example by mx.test with ESMTPS id 2\r\n\
                From: Alice <alice@example.com>\r\n\
                To: Bob <bob@example.com>\r\n\
                Subject: test\r\n\
                Date: Mon, 23 May 2026 12:00:00 +0000\r\n\
                Message-ID: <abc@example.com>\r\n\
                MIME-Version: 1.0\r\n\
                Content-Type: text/plain; charset=utf-8\r\n\
                \r\n\
                Hello world\r\n";

    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let msg = Message::new(raw);
        std::hint::black_box(msg.header("Subject"));
        std::hint::black_box(msg.header("From"));
        std::hint::black_box(msg.body_offset());
        // Walk Received chain — this is the path the server uses for
        // loop detection (RFC 5321 §4.4).
        for h in msg.headers() {
            std::hint::black_box(h.value_str());
        }
    }
    let elapsed = start.elapsed();
    let per_lookup = elapsed / iterations;
    assert!(
        per_lookup < Duration::from_micros(5),
        "rfc5322 lookup chain per-msg {per_lookup:?} (budget: 5 µs)"
    );
}

#[test]
fn dmarc_evaluate_chain_under_budget() {
    // Budget: full DMARC eval the server does per-message:
    //   1. Parse the policy TXT record.
    //   2. Run align + eval given SPF/DKIM outcomes.
    // On clean Mac this is < 1 µs; budget at 20 µs leaves ~20× headroom.
    let txt = "v=DMARC1; p=reject; sp=quarantine; adkim=s; aspf=r; \
               pct=100; rua=mailto:aggregate@example.com";

    let input = DmarcInput {
        from_domain: "example.com".into(),
        policy_domain: "example.com".into(),
        spf: Some(SpfResult {
            domain: "mail.example.com".into(),
            pass: true,
        }),
        dkim: vec![DkimSignatureResult {
            d_domain: "example.com".into(),
            pass: true,
        }],
    };

    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let policy = DmarcPolicy::parse(txt).expect("DmarcPolicy::parse");
        let outcome = evaluate(&policy, &input);
        std::hint::black_box(outcome);
    }
    let elapsed = start.elapsed();
    let per_eval = elapsed / iterations;
    assert!(
        per_eval < Duration::from_micros(20),
        "DMARC eval chain per-msg {per_eval:?} (budget: 20 µs)"
    );
}

#[test]
fn smtp_command_parse_recipe_under_budget() {
    // Budget: parsing the 4 commands of a typical SMTP envelope. This is
    // the wire-format parse cost the server pays per inbound connection.
    // On clean Mac sub-300 ns total; budget at 5 µs.
    let envelope = [
        "EHLO mail.example.com",
        "MAIL FROM:<sender@example.com> SIZE=10240",
        "RCPT TO:<recipient1@example.com>",
        "RCPT TO:<recipient2@example.com>",
        "DATA",
    ];

    let iterations = 20_000;
    let start = Instant::now();
    for _ in 0..iterations {
        for line in &envelope {
            let parsed: Command = parse_command(line).expect("parse_command");
            std::hint::black_box(parsed);
        }
    }
    let elapsed = start.elapsed();
    let per_envelope = elapsed / iterations;
    assert!(
        per_envelope < Duration::from_micros(5),
        "SMTP command-parse recipe per-envelope {per_envelope:?} (budget: 5 µs)"
    );
}
