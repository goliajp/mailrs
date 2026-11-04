//! Given the conversations held now, compare reports what a re-judge after
//! activating would newly hold: two packs can agree on every message and a
//! re-judge still hold conversations, when a message both hold sits in a
//! conversation that is not held today.
use std::{fs, process::Command};

#[test]
fn compare_counts_holds_in_conversations_not_held_now() {
    let root = std::env::temp_dir().join(format!("mailrs-fraud-check-held-{}", std::process::id()));
    let maildir = root.join("golia.jp/lihao/cur");
    fs::create_dir_all(&maildir).unwrap();
    // a reserved sender domain is hold-grade in the built-in pack
    for (i, mid) in ["held-root@a", "free-root@a"].iter().enumerate() {
        fs::write(
            maildir.join(i.to_string()),
            format!(
                "Received: from a (1.2.3.4:5)\r\n\tby mail.golia.ai with ESMTP\r\n\tfor <lihao@golia.jp>; Mon, 12 Oct 2026 18:40:33 +0000\r\nFrom: x <x@example.org>\r\nMessage-ID: <{mid}>\r\nSubject: hello\r\nDate: Mon, 12 Oct 2026 18:40:00 +0000\r\n\r\nHello\r\n"
            ),
        )
        .unwrap();
    }
    let held = root.join("held.tsv");
    fs::write(&held, "lihao@golia.jp\theld-root@a\n").unwrap();
    let released = root.join("released.tsv");
    fs::write(&released, "lihao@golia.jp\tfree-root@a\n").unwrap();
    let list = root.join("list.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args([
            "compare",
            "builtin",
            "builtin",
            // the maildir root, so a file's path says whose it is
            root.to_str().unwrap(),
            "2",
            "--list",
            list.to_str().unwrap(),
            "--held-threads",
            held.to_str().unwrap(),
            "--released-threads",
            released.to_str().unwrap(),
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
    assert_eq!(
        report["changed"], 0,
        "the same pack on both sides: {report}"
    );
    assert_eq!(
        report["would_hold_in_unheld_threads"],
        serde_json::json!({"messages": 1, "threads": 1, "person_released_threads": 1}),
        "{report}"
    );
    let now: Vec<bool> = fs::read_to_string(&list)
        .unwrap()
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["thread_held_now"]
                .as_bool()
                .unwrap()
        })
        .collect();
    assert_eq!(now, vec![true, false]);

    // without the thread file, the report is as it was
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args(["compare", "builtin", "builtin", root.to_str().unwrap(), "2"])
        .env_remove("MAILRS_KEVY_URL")
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        report.get("would_hold_in_unheld_threads").is_none(),
        "{report}"
    );
    fs::remove_dir_all(&root).unwrap();
}
