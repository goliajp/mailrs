//! The real worker binary under the pool: answers, each way it can fail
//! on one input, and what the next request finds.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mailrs_parse_worker::proto::{AnswerHeader, Op};
use mailrs_parse_worker::{Failure, Kind, Parser, Pool, Setup, Want};

const CLEAN: &[u8] = b"From: a@b.example\r\nSubject: hi\r\nContent-Type: text/html\r\n\r\n<p>hello <b>there</b></p>\r\n";

fn pool(workers: usize) -> Pool {
    Pool::new(Setup {
        bin: env!("CARGO_BIN_EXE_mailrs-parse-worker").into(),
        workers,
        env: vec![("MAILRS_PARSE_WORKER_FAULTS".into(), "1".into())],
    })
}

fn faulty(what: &str) -> Vec<u8> {
    format!("From: a@b.example\r\nX-Parse-Worker-Fault: {what}\r\n\r\nbody\r\n").into_bytes()
}

fn body_text() -> Op {
    Op::Message(Want {
        body_text: true,
        ..Want::default()
    })
}

fn text_of(pool: &Pool, raw: &[u8]) -> Result<Option<String>, Failure> {
    pool.call(&body_text(), raw, Duration::from_secs(5))
        .map(|a| match a.header {
            AnswerHeader::Message(r) => r.body_text,
            other => panic!("{other:?}"),
        })
}

#[test]
fn the_worker_answers_what_the_parser_answers() {
    let p = pool(1);
    let want = mailrs_clean::body_text_for_search(&mailrs_mime::parse(CLEAN));
    assert!(want.as_deref().is_some_and(|t| t.contains("hello")));
    for _ in 0..3 {
        assert_eq!(text_of(&p, CLEAN).unwrap(), want);
    }
}

#[test]
fn a_worker_that_aborts_is_a_panic_on_that_input_and_the_next_one_is_read() {
    let p = pool(1);
    assert_eq!(
        text_of(&p, &faulty("abort")),
        Err(Failure::OnInput {
            parser: Parser::Html2text,
            kind: Kind::Panic
        })
    );
    assert!(text_of(&p, CLEAN).unwrap().is_some(), "restarted at once");
}

#[test]
fn a_worker_past_its_limit_is_killed_as_a_timeout() {
    let p = pool(1);
    // started first: the limit is for the parse, not for exec
    assert!(text_of(&p, CLEAN).unwrap().is_some());
    let started = Instant::now();
    let got = p.call(&body_text(), &faulty("hang"), Duration::from_millis(300));
    assert_eq!(
        got.err(),
        Some(Failure::OnInput {
            parser: Parser::Html2text,
            kind: Kind::Timeout
        })
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(text_of(&p, CLEAN).unwrap().is_some());
}

#[test]
fn an_exit_or_a_garbled_answer_is_an_error_on_that_input() {
    let p = pool(1);
    for what in ["exit", "garbage"] {
        let got = text_of(&p, &faulty(what));
        assert!(
            matches!(
                got,
                Err(Failure::OnInput {
                    kind: Kind::Error,
                    ..
                })
            ),
            "{what}: {got:?}"
        );
        assert!(text_of(&p, CLEAN).unwrap().is_some());
    }
}

#[test]
fn a_worker_killed_from_outside_mid_message_is_killed_on_that_input() {
    let p = Arc::new(pool(1));
    assert!(text_of(&p, CLEAN).unwrap().is_some());
    let caller = {
        let p = p.clone();
        std::thread::spawn(move || p.call(&body_text(), &faulty("hang"), Duration::from_secs(30)))
    };
    // the pid is this pool's own child, recorded when it started
    let started = Instant::now();
    let pid = loop {
        if let Some(pid) = p.worker_pids().first().copied() {
            break pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no worker started"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    std::thread::sleep(Duration::from_millis(500));
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(killed);
    assert_eq!(
        caller.join().unwrap().err(),
        Some(Failure::OnInput {
            parser: Parser::Html2text,
            kind: Kind::Killed
        })
    );
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn a_worker_that_keeps_dying_pauses_and_says_unavailable_not_failed() {
    let p = pool(1);
    assert!(matches!(
        text_of(&p, &faulty("abort")),
        Err(Failure::OnInput { .. })
    ));
    assert!(matches!(
        text_of(&p, &faulty("abort")),
        Err(Failure::OnInput { .. })
    ));
    // two deaths in a row: a pause, during which nothing is blamed
    assert_eq!(text_of(&p, CLEAN), Err(Failure::Unavailable));
    std::thread::sleep(Duration::from_millis(1100));
    assert!(
        text_of(&p, CLEAN).unwrap().is_some(),
        "back after the pause"
    );
}

#[test]
fn a_missing_binary_is_unavailable() {
    let p = Pool::new(Setup {
        bin: "/nonexistent/mailrs-parse-worker".into(),
        workers: 1,
        env: Vec::new(),
    });
    assert_eq!(text_of(&p, CLEAN), Err(Failure::Unavailable));
}

#[test]
fn report_payloads_come_back_as_the_bytes_the_parser_gives() {
    let p = pool(1);
    let raw = [
        b"From: r@b.example\r\nContent-Type: multipart/mixed; boundary=z\r\n\r\n--z\r\n\
Content-Type: text/xml; name=r.xml\r\nContent-Disposition: attachment; filename=r.xml\r\n\r\n"
            .as_slice(),
        b"<feedback><x>\xff</x></feedback>",
        b"\r\n--z--\r\n",
    ]
    .concat();
    let want = mailrs_dmarc::ingest::extract_report_payloads(&raw);
    assert!(!want.is_empty(), "the fixture carries a report");
    let got = p
        .call(&Op::ReportPayloads, &raw, Duration::from_secs(5))
        .unwrap();
    assert_eq!(got.header, AnswerHeader::ReportPayloads);
    assert_eq!(got.blobs, want);
}

#[test]
fn a_one_off_worker_answers_and_is_gone() {
    let p = pool(1);
    let got = p
        .call_once(&body_text(), CLEAN, Duration::from_secs(5))
        .unwrap();
    assert!(matches!(got.header, AnswerHeader::Message(_)));
    assert!(
        p.worker_pids().is_empty(),
        "the resident slots were not used"
    );
}
