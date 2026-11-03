//! A message filed without passing the receiver, carrying an
//! `Authentication-Results:` in this server's name: each core files it
//! without that header, and the stored re-judge does not read a DKIM pass
//! for the sender out of it.

use std::path::Path;

use super::ALICE;
use super::harness::{AUTHSERV, Cores, Diffs};

/// Our name on top, our trace below it, and a pass for a domain the
/// forger does not control: what our receiver would have written.
fn forged(message_id: &str, from: &str) -> Vec<u8> {
    format!(
        "Authentication-Results: {AUTHSERV};\r\n\tspf=pass;\r\n\tdkim=pass header.d=stripe.com header.s=s1;\r\n\tdmarc=pass\r\n\
Received: from mx.stripe.com (1.2.3.4:5)\r\n\tby {AUTHSERV} with ESMTP\r\n\tfor <{ALICE}>; Mon, 2 Nov 2026 00:00:00 +0000\r\n\
DKIM-Signature: v=1; a=rsa-sha256; d=stripe.com; s=s1; h=from; bh=x; b=y\r\n\
DKIM-Signature: v=1; a=rsa-sha256; d=amazonses.com; s=ug7; h=from; bh=x; b=y\r\n\
Date: Mon, 02 Nov 2026 00:00:00 +0000\r\n\
From: {from}\r\n\
Reply-To: billing@runpod.io\r\n\
To: Alice <{ALICE}>\r\n\
Subject: Your receipt\r\n\
Message-ID: <{message_id}>\r\n\
\r\n\
Thanks.\r\n"
    )
    .into_bytes()
}

/// The file under `root` holding `message_id`.
fn stored(root: &Path, message_id: &str) -> Option<Vec<u8>> {
    let needle = format!("Message-ID: <{message_id}>");
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let raw = std::fs::read(&path).ok()?;
            if String::from_utf8_lossy(&raw).contains(&needle) {
                return Some(raw);
            }
        }
    }
    None
}

/// `dkim_aligned` as the stored re-judge computes it for `raw`.
fn aligned(raw: &[u8]) -> bool {
    let probe = format!(
        "{}\nrule('probe-aligned', 'content', 1, false, function(m) if m.dkim_aligned then return 'aligned' end end)\n",
        mailrs_fraud_lua::DEFAULT_SOURCE
    );
    let reading = mailrs_fraud_lua::stored::Reading::of(raw, Some(AUTHSERV), &[]);
    let out = mailrs_fraud_lua::Rules::compile(&probe)
        .expect("compiles")
        .classify(
            &reading.facts(0, 0, Default::default()),
            &Default::default(),
        );
    out.findings.has("probe-aligned")
}

fn check(c: &Cores, d: &mut Diffs, what: &str, message_id: &str) {
    for (core, root) in [
        ("fastcore", c.fastcore_state.maildir_root.as_str()),
        ("pg-core", c.pg_core_state.maildir_root.as_str()),
    ] {
        let Some(raw) = stored(Path::new(root), message_id) else {
            d.0.push(format!("{what}: {core} filed no file for {message_id}"));
            continue;
        };
        let text = String::from_utf8_lossy(&raw);
        let head = text.split("\r\n\r\n").next().unwrap_or_default();
        if head.contains(&format!("Authentication-Results: {AUTHSERV}")) {
            d.0.push(format!(
                "{what}: {core} filed the header naming it:\n{head}"
            ));
        }
        if aligned(&raw) {
            d.0.push(format!(
                "{what}: {core}'s stored copy reads as a DKIM pass for stripe.com"
            ));
        }
    }
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let (fc, pg) = (&c.fastcore, &c.pg_core);
    let raw = forged("forged-import@stripe.test", "Stripe <invoice@stripe.com>");
    d.both(
        "import_message carrying a results header in this server's name",
        fc.import_message(ALICE, &raw, None).await,
        pg.import_message(ALICE, &raw, None).await,
        &["blob_ref"],
    );
    check(c, d, "import", "forged-import@stripe.test");

    let raw = forged("forged-sent@example.test", &format!("Alice <{ALICE}>"));
    d.both(
        "file_sent carrying a results header in this server's name",
        fc.file_sent(ALICE, &raw, &[]).await,
        pg.file_sent(ALICE, &raw, &[]).await,
        &["blob_ref"],
    );
    check(c, d, "sent copy", "forged-sent@example.test");
}
