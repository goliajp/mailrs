//! An APPENDed message carrying an `Authentication-Results:` in this
//! server's name is filed without it, on both cores, and the stored
//! re-judge reads no DKIM pass for the sender out of it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mailrs_mail_protocols::backend::Directory;

use super::checks::{Client, code};
use super::harness::{HOST, USER};

const MESSAGE_ID: &str = "forged-append@stripe.test";

/// What our receiver would have written on top, written by the client.
fn forged() -> String {
    format!(
        "Authentication-Results: {HOST};\r\n\tdkim=pass header.d=stripe.com header.s=s1\r\n\
Received: from mx.stripe.com (1.2.3.4:5)\r\n\tby {HOST} with ESMTP\r\n\tfor <{USER}>; Mon, 2 Nov 2026 00:00:00 +0000\r\n\
DKIM-Signature: v=1; a=rsa-sha256; d=stripe.com; s=s1; h=from; bh=x; b=y\r\n\
DKIM-Signature: v=1; a=rsa-sha256; d=amazonses.com; s=ug7; h=from; bh=x; b=y\r\n\
From: Stripe <invoice@stripe.com>\r\nReply-To: billing@runpod.io\r\nTo: {USER}\r\n\
Subject: forged\r\nMessage-ID: <{MESSAGE_ID}>\r\n\r\nThanks.\r\n"
    )
}

fn on_disk(home: &Path) -> Option<Vec<u8>> {
    let needle = format!("Message-ID: <{MESSAGE_ID}>");
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(home.join(leaf)).ok())
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .find(|b| String::from_utf8_lossy(b).contains(&needle))
}

/// `dkim_aligned` as the stored re-judge computes it for `raw`.
fn aligned(raw: &[u8]) -> bool {
    let probe = format!(
        "{}\nrule('probe-aligned', 'content', 1, false, function(m) if m.dkim_aligned then return 'aligned' end end)\n",
        mailrs_fraud_lua::DEFAULT_SOURCE
    );
    let reading = mailrs_fraud_lua::stored::Reading::of(raw, Some(HOST), &[]);
    mailrs_fraud_lua::Rules::compile(&probe)
        .expect("compiles")
        .classify(
            &reading.facts(0, 0, Default::default()),
            &Default::default(),
        )
        .findings
        .has("probe-aligned")
}

pub(super) async fn append<D: Directory>(dir: Arc<D>, home: PathBuf) -> Vec<String> {
    let mut diffs = Vec::new();
    let mut c = Client::imap(dir).await;
    let r = c.append("f1", "INBOX", "()", &forged()).await;
    if code(&r, "[APPENDUID ").is_none() {
        diffs.push(format!("forged APPEND: not filed\n    got: {r:?}"));
        return diffs;
    }
    let Some(raw) = on_disk(&home) else {
        diffs.push("forged APPEND: no file in INBOX".into());
        return diffs;
    };
    let text = String::from_utf8_lossy(&raw);
    let head = text.split("\r\n\r\n").next().unwrap_or_default();
    if head.contains(&format!("Authentication-Results: {HOST}")) {
        diffs.push(format!(
            "forged APPEND: the header naming this server was filed\n    got: {head}"
        ));
    }
    if aligned(&raw) {
        diffs.push("forged APPEND: the stored copy reads as a DKIM pass for stripe.com".into());
    }
    diffs
}
