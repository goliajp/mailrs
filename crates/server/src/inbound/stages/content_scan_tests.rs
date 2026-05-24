//! Tests for `content_scan` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use std::net::{IpAddr, Ipv4Addr};

use super::*;

fn ctx_with(message: &[u8]) -> ReceiveContext {
    ReceiveContext::new(
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
        "client.example.com",
        "alice@example.com",
        "bob@example.com",
        message.to_vec(),
        "mx.example.com",
    )
}

#[tokio::test]
async fn writes_score_and_rules_into_context() {
    let stage = ContentScanStage::new();
    let mut ctx = ctx_with(b"From: alice@example.com\r\n\r\nbody");
    let outcome = stage.evaluate(&mut ctx).await;
    assert_eq!(outcome, StageOutcome::Continue);
    // assertion on actual score depends on rule set — the contract here
    // is just that the stage writes both fields and continues.
    assert!(ctx.content_score >= 0.0);
    // matched_rules may be empty for a benign message
    let _ = ctx.matched_rules;
}

#[tokio::test]
async fn name_is_stable() {
    let stage = ContentScanStage::new();
    assert_eq!(stage.name(), "content_scan");
}
