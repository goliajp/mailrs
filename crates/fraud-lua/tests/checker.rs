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
