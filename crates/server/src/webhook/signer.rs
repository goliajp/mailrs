//! Adapter: server uses `mailrs-webhook-signature` (1.0.0) for HMAC
//! payload signing. This file is a thin re-export shim so worker.rs's
//! existing `signer::sign_payload` / `signer::format_signature_header`
//! call sites resolve unchanged.

/// HMAC-SHA256 sign a payload. Wrapper for back-compat with the old
/// internal API name.
pub fn sign_payload(secret: &[u8], payload: &[u8]) -> String {
    mailrs_webhook_signature::sign(secret, payload)
}

/// Format a hex signature into the `sha256=<hex>` header value.
/// Wrapper for back-compat with the old internal API name.
pub fn format_signature_header(signature: &str) -> String {
    mailrs_webhook_signature::format_header(signature)
}
