//! `has_unreadable_archive` reaches the rules from whole messages, on the
//! sweep's reading and on the checker's, and a program inside a RAR holds
//! whatever the RAR is called. The archives are headers only.
use std::{fs, process::Command};

use mailrs_fraud::Policy;
use mailrs_fraud_lua::stored::Reading;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

/// A hold-level probe, so the checker's per-rule counts show the fact.
const PROBE: &str = "\n\
rule('probe-unreadable-archive', 'content', 6, true, function(m)\n\
  if m.has_unreadable_archive then return 'seen' end\n\
end)\n";

fn vint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

fn block(body: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 4];
    out.extend(vint(body.len() as u64));
    out.extend_from_slice(body);
    out
}

/// A RAR 5 archive naming `members`, no member data.
fn rar5(members: &[&str]) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    out.extend(block(&[1, 0, 0]));
    for name in members {
        let mut body = vec![2, 0x02, 0, 0, 0, 0x20, 0, 0];
        body.extend(vint(name.len() as u64));
        body.extend_from_slice(name.as_bytes());
        out.extend(block(&body));
    }
    out.extend(block(&[5, 0, 0]));
    out
}

/// A RAR 5 archive with header encryption: nothing past the signature
/// says what it holds.
fn rar5_encrypted_headers() -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    out.extend(block(&[4, 0, 0, 0]));
    out.extend_from_slice(&[0xee; 64]);
    out
}

fn seven_z() -> Vec<u8> {
    let mut out = b"7z\xbc\xaf\x27\x1c\x00\x04".to_vec();
    out.extend_from_slice(&[0u8; 24]);
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

fn message(filename: &str, archive: &[u8]) -> Vec<u8> {
    format!(
        "From: Orders <orders@vendorship.com>\r\nTo: sales@golia.jp\r\n\
Subject: order\r\nDate: Tue, 15 Sep 2026 09:00:00 +0000\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nPlease see attached.\r\n\
--b\r\nContent-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename=\"{filename}\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\n{}\r\n--b--\r\n",
        base64(archive)
    )
    .into_bytes()
}

/// The rules that hold, judged on the sweep's reading of `raw`.
fn holds(raw: &[u8]) -> Vec<String> {
    let reading = Reading::of(raw, None, &[]);
    let mut rules = Rules::compile(&format!("{DEFAULT_SOURCE}{PROBE}")).expect("compiles");
    let out = rules.classify(&reading.facts(0, 0, Default::default()), &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let mut held: Vec<String> = out.findings.rules().iter().map(|r| r.to_string()).collect();
    held.sort();
    held
}

#[test]
fn the_sweep_reads_what_a_rar_or_a_7z_holds() {
    // the shape of the history: a script inside, attached as `.taz`
    assert_eq!(
        holds(&message("order.taz", &rar5(&["order.bat"]))),
        ["executable-attachment"]
    );
    assert!(holds(&message("docs.rar", &rar5(&["a.pdf", "b.xlsx"]))).is_empty());
    assert_eq!(
        holds(&message("invoice.rar", &rar5_encrypted_headers())),
        ["probe-unreadable-archive"]
    );
    assert_eq!(
        holds(&message("invoice.7z", &seven_z())),
        ["probe-unreadable-archive"]
    );
}

#[test]
fn the_checker_reads_the_fact() {
    let root = std::env::temp_dir().join(format!("mailrs-fraud-check-b58-{}", std::process::id()));
    fs::create_dir_all(root.join("cur")).unwrap();
    for (i, raw) in [
        message("order.taz", &rar5(&["order.bat"])),
        message("docs.rar", &rar5(&["a.pdf"])),
        message("invoice.rar", &rar5_encrypted_headers()),
        message("invoice.7z", &seven_z()),
    ]
    .iter()
    .enumerate()
    {
        fs::write(root.join("cur").join(i.to_string()), raw).unwrap();
    }
    let candidate = root.join("candidate.lua");
    fs::write(&candidate, format!("{DEFAULT_SOURCE}{PROBE}")).unwrap();
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
        "probe-unreadable-archive": 2,
    });
    assert_eq!(report["holds_by_rule"], candidate_holds, "{report}");
    let baseline = serde_json::json!({"executable-attachment": 1});
    assert_eq!(report["baseline_holds_by_rule"], baseline, "{report}");
    fs::remove_dir_all(root).unwrap();
}
