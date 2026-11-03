//! The compare command's per-message list and its full tally of what
//! changed, by rule: the summary's samples stop at fifty.
use std::{fs, process::Command};

#[test]
fn checker_lists_every_message_and_tallies_each_changed_rule() {
    let root = std::env::temp_dir().join(format!("mailrs-fraud-check-list-{}", std::process::id()));
    let maildir = root.join("golia.jp/lihao/cur");
    fs::create_dir_all(&maildir).unwrap();
    for (i, date) in [
        "Thu, 12 Nov 2026 18:40:32 +0000",
        "Fri, 13 Nov 2026 18:40:32 +0000",
        "Mon, 12 Oct 2026 18:40:00 +0000",
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            maildir.join(i.to_string()),
            format!(
                "Received: from a (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <lihao@golia.jp>; Mon, 12 Oct 2026 18:40:33 +0000\r\nFrom: base2026 <base2026@tintgate.com>\r\nSubject: hello\r\nDate: {date}\r\n\r\nHello\r\n"
            ),
        )
        .unwrap();
    }
    let rule = "m.date_ahead > 2 * 86400";
    let baseline = root.join("baseline.lua");
    fs::write(
        &baseline,
        mailrs_fraud_lua::DEFAULT_SOURCE.replace(rule, "false"),
    )
    .unwrap();
    let list = root.join("list.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args([
            "compare",
            baseline.to_str().unwrap(),
            "builtin",
            root.join("golia.jp").to_str().unwrap(),
            "3",
            "--list",
            list.to_str().unwrap(),
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
    assert_eq!(report["changed"], 2, "{report}");
    assert_eq!(
        report["changed_by_rule"],
        serde_json::json!({"+dated-in-the-future": 2}),
        "{report}"
    );
    let lines: Vec<serde_json::Value> = fs::read_to_string(&list)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3, "one line per checked message");
    let first = &lines[0];
    assert_eq!(
        first["path"], "lihao/cur/0",
        "relative to the maildir argument"
    );
    assert_eq!(first["rules_before"], serde_json::json!([]));
    assert_eq!(
        first["rules_after"],
        serde_json::json!(["dated-in-the-future"])
    );
    assert_eq!(first["held_before"], false);
    assert_eq!(first["held_after"], true);
    assert_eq!(lines[2]["rules_after"], serde_json::json!([]));
    fs::remove_dir_all(root).unwrap();
}
