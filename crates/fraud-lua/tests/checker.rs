//! Exercise the actual command used by rule publication, with raw MIME input.
use std::{fs, process::Command};

#[test]
fn checker_compares_raw_mail_and_refuses_an_incomplete_corpus() {
    let root = std::env::temp_dir().join(format!("mailrs-fraud-check-{}", std::process::id()));
    fs::create_dir_all(root.join("cur")).unwrap();
    for (i, from) in [
        "ChatGPT <admin@heavenerandassociates.com>",
        "OpenAI <noreply@tm.openai.com>",
        "Alice <a@example.com>",
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            root.join("cur").join(i.to_string()),
            format!(
                "From: {from}\r\nTo: sales@golia.jp\r\nSubject: payment update\r\n\r\nHello\r\n"
            ),
        )
        .unwrap();
    }
    let command = |limit: &str| {
        Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
            .args(["compare", "rust", "builtin", root.to_str().unwrap(), limit])
            .env_remove("MAILRS_KEVY_URL")
            .output()
            .unwrap()
    };
    let ok = command("3");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(report["checked"], 3);
    assert_eq!(report["changed"], 0);
    assert_eq!(report["candidate_holds"], 1);
    assert!(!command("2").status.success());
    fs::remove_dir_all(root).unwrap();
}

// the dry run before publishing must see the date: without it the
// checker reports a rule on dates as changing nothing
#[test]
fn checker_sees_a_letter_dated_ahead_of_its_arrival() {
    let root = std::env::temp_dir().join(format!("mailrs-fraud-check-date-{}", std::process::id()));
    fs::create_dir_all(root.join("cur")).unwrap();
    for (i, date) in [
        "Thu, 12 Nov 2026 18:40:32 +0000",
        "Mon, 12 Oct 2026 18:40:00 +0000",
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            root.join("cur").join(i.to_string()),
            format!(
                "Received: from a (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <lihao@golia.jp>; Mon, 12 Oct 2026 18:40:33 +0000\r\nFrom: base2026 <base2026@tintgate.com>\r\nSubject: hello\r\nDate: {date}\r\n\r\nHello\r\n"
            ),
        )
        .unwrap();
    }
    // the bundle as it was before the rule: the same source with it disarmed
    let rule = "m.date_ahead > 2 * 86400";
    assert!(mailrs_fraud_lua::DEFAULT_SOURCE.contains(rule));
    let baseline = root.join("baseline.lua");
    fs::write(
        &baseline,
        mailrs_fraud_lua::DEFAULT_SOURCE.replace(rule, "false"),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args([
            "compare",
            baseline.to_str().unwrap(),
            "builtin",
            root.to_str().unwrap(),
            "2",
        ])
        .env_remove("MAILRS_KEVY_URL")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["changed"], 1, "{report}");
    assert_eq!(report["candidate_holds"], 1, "{report}");
    fs::remove_dir_all(root).unwrap();
}
