# mailrs-apns

Token-based APNs client: ES256 provider JWTs (cached and refreshed on
Apple's schedule), HTTP/2 delivery via reqwest, and `410` /
`BadDeviceToken` surfaced as `Outcome::Unregistered` so callers can prune
dead tokens.

```rust
let client = mailrs_apns::ApnsClient::new(
    &std::fs::read_to_string("AuthKey_ABC123.p8")?,
    "ABC123",          // key id
    "KF79DRC524",      // team id
    "jp.golia.mailrs", // topic = bundle id
    mailrs_apns::SANDBOX_ENDPOINT,
)?;
match client.send_alert(&device_token, "alice@example.com", "Quarterly report").await? {
    mailrs_apns::Outcome::Unregistered => { /* delete the token */ }
    _ => {}
}
```

The endpoint is an argument, not a constant: the sandbox gateway (where
Xcode debug builds' tokens live) and a local test stub are the same code
path as production. A sandbox token pushed at production answers
`BadDeviceToken` — indistinguishable from a rotten token — so the
environment split matters more than it looks.
