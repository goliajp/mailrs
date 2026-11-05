//! A RAR or a 7z received over SMTP, with the fraud scan on: a program
//! inside a RAR holds whatever the RAR is called, an archive whose names
//! cannot be read reaches the rules as `has_unreadable_archive`, and a 7z
//! is counted where `/api/health` reads it. Archives are headers only.

use mailrs_core_sidestate::families::fraud_hits::{SEVEN_Z_ATTACHMENT, counts};
use mailrs_receiver::kevy_net::KevyNetClient;

use crate::harness::{Client, REAL, SPF_DOMAIN, files_under, receiver_with};

const KEVY: &str = "mem://receiver-rar-and-7z";

/// A score-only probe beside the built-in rules: it shows the fact
/// arrived without changing whether anything is held.
const PROBE: &str = "\n\
rule('probe-unreadable-archive', 'content', 1, false, function(m)\n\
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

fn rar5(name: &str) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    out.extend(block(&[1, 0, 0]));
    let mut body = vec![2, 0x02, 0, 0, 0, 0x20, 0, 0];
    body.extend(vint(name.len() as u64));
    body.extend_from_slice(name.as_bytes());
    out.extend(block(&body));
    out.extend(block(&[5, 0, 0]));
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

/// Send one message carrying `archive` as `filename`.
async fn send(port: u16, filename: &str, archive: &[u8]) {
    let mut c = Client::connect(port).await;
    let from = format!("orders@{SPF_DOMAIN}");
    assert!(
        c.say(&format!("MAIL FROM:<{from}>"))
            .await
            .starts_with("250")
    );
    let r = c.say(&format!("RCPT TO:<{REAL}>")).await;
    assert!(r.starts_with("250"), "{r}");
    assert!(c.say("DATA").await.starts_with("354"));
    let r = c
        .say(&format!(
            "From: Orders <{from}>\r\nTo: {REAL}\r\nSubject: order\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nPlease see attached.\r\n\
--b\r\nContent-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename=\"{filename}\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\n{}\r\n--b--\r\n.",
            base64(archive)
        ))
        .await;
    assert!(r.starts_with("250"), "{r}");
}

fn verdicts(spool: &std::path::Path) -> Vec<serde_json::Value> {
    files_under(spool)
        .iter()
        .map(|blob| {
            let (envelope, _) = mailrs_core::spool::decode_spool_blob(blob).unwrap();
            envelope
                .fraud_verdict
                .map(|v| serde_json::from_str(&v).unwrap())
                .unwrap_or(serde_json::Value::Null)
        })
        .collect()
}

#[tokio::test]
async fn a_rar_and_a_7z_are_read_on_the_receive_path() {
    let pack = std::env::temp_dir().join(format!("rar-and-7z-{}.lua", std::process::id()));
    std::fs::write(
        &pack,
        format!("{}{PROBE}", mailrs_fraud_lua::DEFAULT_SOURCE),
    )
    .unwrap();
    // Read once per thread when the first scan runs; no other test in
    // this binary scans, and this one scans only on its own thread.
    unsafe { std::env::set_var("MAILRS_FRAUD_RULES_FILE", &pack) };

    let rx = receiver_with("rar-and-7z", Some(KEVY), true).await;
    send(rx.port, "order.taz", &rar5("order.bat")).await;
    let held = verdicts(&rx.spool);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0]["quarantined"], true, "{}", held[0]);

    let rx7 = receiver_with("rar-and-7z-seven", Some(KEVY), true).await;
    send(rx7.port, "invoice.7z", &seven_z()).await;
    let scored = verdicts(&rx7.spool);
    assert_eq!(scored.len(), 1);
    assert_eq!(scored[0]["quarantined"], false, "{}", scored[0]);

    // The verdict names no rule; the receiver's per-rule hit counters,
    // which `/api/health` reads, do.
    let client = std::sync::Arc::new(KevyNetClient::new(KEVY));
    let now = chrono::Utc::now().timestamp();
    let names: Vec<String> = [
        "executable-attachment",
        "probe-unreadable-archive",
        SEVEN_Z_ATTACHMENT,
    ]
    .map(String::from)
    .to_vec();
    let got = tokio::task::spawn_blocking(move || client.with_conn(|c| counts(c, &names, now)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.today["executable-attachment"], 1, "the RAR holds");
    assert_eq!(
        got.today["probe-unreadable-archive"], 1,
        "the 7z, and not the RAR whose program is the stronger fact"
    );
    assert_eq!(
        got.today[SEVEN_Z_ATTACHMENT], 1,
        "one 7z, the RAR not counted"
    );
    let _ = std::fs::remove_file(pack);
}
