//! `has_encrypted_document_archive` and `reply_marker_without_thread`
//! reach the rules from whole messages, on the sweep's reading and on the
//! checker's, and an encrypted archive of documents no longer reads as
//! code.
use std::{fs, process::Command};

use mailrs_fraud::Policy;
use mailrs_fraud_lua::stored::Reading;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

/// Two hold-level probes, so the checker's per-rule counts show them.
const PROBES: &str = "\n\
rule('probe-encrypted-documents', 'content', 6, true, function(m)\n\
  if m.has_encrypted_document_archive then return 'seen' end\n\
end)\n\
rule('probe-reply-without-thread', 'identity', 6, true, function(m)\n\
  if m.reply_marker_without_thread then return 'seen' end\n\
end)\n";

/// A zip with these `(name, encrypted)` entries, contents empty.
fn zip(entries: &[(&str, bool)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut dir = Vec::new();
    for (name, encrypted) in entries {
        let offset = out.len() as u32;
        let flags: u16 = u16::from(*encrypted);
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&[20, 0]);
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&[0u8; 18]);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        dir.extend_from_slice(b"PK\x01\x02");
        dir.extend_from_slice(&[20, 0, 20, 0]);
        dir.extend_from_slice(&flags.to_le_bytes());
        dir.extend_from_slice(&[0u8; 18]);
        dir.extend_from_slice(&(name.len() as u16).to_le_bytes());
        dir.extend_from_slice(&[0u8; 12]);
        dir.extend_from_slice(&offset.to_le_bytes());
        dir.extend_from_slice(name.as_bytes());
    }
    let dir_at = out.len() as u32;
    let dir_len = dir.len() as u32;
    out.extend_from_slice(&dir);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&dir_len.to_le_bytes());
    out.extend_from_slice(&dir_at.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

fn base64(b: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            match i <= c.len() {
                true => s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char),
                false => s.push('='),
            }
        }
    }
    s
}

fn message(subject: &str, thread: &str, archive: Option<&[(&str, bool)]>) -> Vec<u8> {
    let attachment = archive.map_or(String::new(), |entries| {
        format!(
            "--b\r\nContent-Type: application/zip\r\n\
Content-Disposition: attachment; filename=\"statement.zip\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\n{}\r\n",
            base64(&zip(entries))
        )
    });
    format!(
        "From: Billing <billing@vendorbill.com>\r\nTo: sales@golia.jp\r\n\
Subject: {subject}\r\n{thread}Date: Tue, 15 Sep 2026 09:00:00 +0000\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nThe password is 1234.\r\n{attachment}--b--\r\n"
    )
    .into_bytes()
}

/// The rules that hold, judged on the sweep's reading of `raw`.
fn holds(raw: &[u8]) -> Vec<String> {
    let reading = Reading::of(raw, None, &[]);
    let mut rules = Rules::compile(&format!("{DEFAULT_SOURCE}{PROBES}")).expect("compiles");
    let out = rules.classify(&reading.facts(0, 0, Default::default()), &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let mut held: Vec<String> = out.findings.rules().iter().map(|r| r.to_string()).collect();
    held.sort();
    held
}

#[test]
fn the_sweep_reads_an_encrypted_archive_of_documents() {
    let statement = message("statement", "", Some(&[("statement.pdf", true)]));
    assert_eq!(holds(&statement), ["probe-encrypted-documents"]);
    let script = message("statement", "", Some(&[("statement.hta", true)]));
    assert_eq!(holds(&script), ["executable-attachment"]);
    let open = message("statement", "", Some(&[("statement.pdf", false)]));
    assert!(holds(&open).is_empty());
}

#[test]
fn the_sweep_reads_a_reply_that_names_no_thread() {
    let fake = message("Re：お見積り", "", None);
    assert_eq!(holds(&fake), ["probe-reply-without-thread"]);
    let real = message("Re: quote", "In-Reply-To: <q1@vendorbill.com>\r\n", None);
    assert!(holds(&real).is_empty());
    let plain = message("quote", "", None);
    assert!(holds(&plain).is_empty());
}

#[test]
fn the_checker_reads_both_facts() {
    let root = std::env::temp_dir().join(format!("mailrs-fraud-check-b42-{}", std::process::id()));
    fs::create_dir_all(root.join("cur")).unwrap();
    for (i, raw) in [
        message("statement", "", Some(&[("statement.pdf", true)])),
        message("Re: quote", "", None),
        message("Re: quote", "References: <q1@vendorbill.com>\r\n", None),
        message("statement", "", Some(&[("statement.hta", true)])),
    ]
    .iter()
    .enumerate()
    {
        fs::write(root.join("cur").join(i.to_string()), raw).unwrap();
    }
    let candidate = root.join("candidate.lua");
    fs::write(&candidate, format!("{DEFAULT_SOURCE}{PROBES}")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mailrs-fraud-check"))
        .args([
            "compare",
            "builtin",
            candidate.to_str().unwrap(),
            root.to_str().unwrap(),
            "4",
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
    let candidate_holds = serde_json::json!({
        "executable-attachment": 1,
        "probe-encrypted-documents": 1,
        "probe-reply-without-thread": 1,
    });
    assert_eq!(report["holds_by_rule"], candidate_holds, "{report}");
    let baseline = serde_json::json!({"executable-attachment": 1});
    assert_eq!(report["baseline_holds_by_rule"], baseline, "{report}");
    fs::remove_dir_all(root).unwrap();
}
