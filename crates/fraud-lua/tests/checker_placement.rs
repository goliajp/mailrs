//! `compare` reports what a pack moves between the inbox, Junk and the
//! held list, not only what it holds: a score-only rule moves mail too.
use mailrs_fraud_lua::DEFAULT_SOURCE;
use std::{fs, process::Command};

fn on(subject: &str) -> String {
    format!("function(m) if m.subject == '{subject}' then return '{subject}' end end")
}

#[test]
fn compare_counts_moves_into_and_out_of_junk_and_the_inbox() {
    let root = std::env::temp_dir().join(format!(
        "mailrs-fraud-check-placement-{}",
        std::process::id()
    ));
    let maildir = root.join("golia.jp/lihao/cur");
    fs::create_dir_all(&maildir).unwrap();
    for (i, (from, subject)) in [
        ("Alpha <news@alpha-shop.jp>", "alpha"),
        ("Bravo <news@bravo-shop.jp>", "bravo"),
        ("Charlie <news@charlie-shop.jp>", "charlie"),
        ("Delta <news@delta-shop.jp>", "delta"),
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            maildir.join(i.to_string()),
            format!(
                "From: {from}\r\nTo: lihao@golia.jp\r\nSubject: {subject}\r\nDate: Mon, 02 Nov 2026 09:00:00 +0000\r\n\r\nHello\r\n"
            ),
        )
        .unwrap();
    }
    // alpha: 4 under both, plus 1.5 in the candidate, inbox to Junk;
    // bravo: 6 then 4, Junk to inbox; charlie: held, then nothing
    let baseline = root.join("baseline.lua");
    fs::write(
        &baseline,
        format!(
            "{DEFAULT_SOURCE}\nrule('t-four', 'content', 4, false, {})\nrule('t-six', 'content', 6, false, {})\nrule('t-hold', 'content', 6, true, {})\n",
            on("alpha"),
            on("bravo"),
            on("charlie")
        ),
    )
    .unwrap();
    let candidate = root.join("candidate.lua");
    fs::write(
        &candidate,
        format!(
            "{DEFAULT_SOURCE}\nrule('t-four', 'content', 4, false, {})\nrule('t-push', 'content', 1.5, false, {})\nrule('t-bravo-four', 'content', 4, false, {})\n",
            on("alpha"),
            on("alpha"),
            on("bravo")
        ),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args([
            "compare",
            baseline.to_str().unwrap(),
            candidate.to_str().unwrap(),
            root.join("golia.jp").to_str().unwrap(),
            "4",
            "--list",
            root.join("list.jsonl").to_str().unwrap(),
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
    // the fields that were there before stay
    assert_eq!(report["removed_holds"], 1, "{report}");
    assert_eq!(report["added_holds"], 0, "{report}");
    assert_eq!(report["would_move_to_junk"], 1, "{report}");
    assert_eq!(report["would_move_out_of_junk"], 1, "{report}");
    assert_eq!(report["would_move_to_inbox"], 2, "{report}");
    assert_eq!(report["would_move_out_of_inbox"], 1, "{report}");
    assert_eq!(
        report["out_of_inbox_sender_domains"],
        serde_json::json!(["alpha-shop.jp"]),
        "{report}"
    );
    let by_rule = &report["placement_moves_by_rule"];
    assert_eq!(
        by_rule["to_junk"],
        serde_json::json!({"+t-push": 1}),
        "{report}"
    );
    assert_eq!(
        by_rule["out_of_junk"],
        serde_json::json!({"+t-bravo-four": 1, "-t-six": 1}),
        "{report}"
    );
    assert_eq!(
        by_rule["to_inbox"],
        serde_json::json!({"+t-bravo-four": 1, "-t-hold": 1, "-t-six": 1}),
        "{report}"
    );
    // without the shared store the readers' not-junk lists are not read,
    // and the report says so instead of counting as if none existed
    assert_eq!(report["whitelist_checked"], false, "{report}");
    assert!(
        report["placement_not_modelled"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str().unwrap().starts_with("recipient whitelist")),
        "{report}"
    );

    let placed: Vec<(String, String)> = fs::read_to_string(root.join("list.jsonl"))
        .unwrap()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            (
                v["placement_before"].as_str().unwrap().to_string(),
                v["placement_after"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        placed,
        [
            pair("inbox", "junk"),
            pair("junk", "inbox"),
            pair("held", "inbox"),
            pair("inbox", "inbox"),
        ]
    );

    let saved = root.join("report.json");
    fs::write(&saved, &out.stdout).unwrap();
    let summary = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args(["summary", saved.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(summary.status.success());
    let text = String::from_utf8(summary.stdout).unwrap();
    for line in [
        "would move to Junk: 1",
        "would move out of Junk: 1",
        "would move to inbox: 2",
        "out of inbox, sender domains: alpha-shop.jp",
        "whitelist checked: false",
    ] {
        assert!(text.lines().any(|l| l == line), "{line}\n{text}");
    }
    fs::remove_dir_all(root).unwrap();
}
