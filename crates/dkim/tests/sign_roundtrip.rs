//! End-to-end DKIM sign → verify roundtrip.
//!
//! Generate a real RSA-2048 / Ed25519 keypair, sign a message with
//! `mailrs_dkim::sign`, then verify it via `mailrs_dkim::verify_all`
//! against a resolver that returns the matching public key TXT.
//! The signature MUST validate — if sign and verify disagree, every
//! signature this crate produces is unusable.

use base64::Engine as _;
use rsa::pkcs8::EncodePublicKey;
use rsa::{RsaPrivateKey, RsaPublicKey};

use mailrs_dkim::{Canon, DkimResolver, DkimResult, DkimSigningKey, SignOpts, sign, verify_all};

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn rsa_pubkey_txt(public_key: &RsaPublicKey) -> String {
    let der = public_key.to_public_key_der().unwrap();
    format!("v=DKIM1; k=rsa; p={}", b64(der.as_bytes()))
}

fn ed25519_pubkey_txt(verifying: &ed25519_dalek::VerifyingKey) -> String {
    // RFC 8463 §3: the p= payload is the raw 32-byte public key in
    // base64, NOT PKCS8.
    format!("v=DKIM1; k=ed25519; p={}", b64(verifying.as_bytes()))
}

struct OneShotResolver {
    txt: String,
}

#[async_trait::async_trait]
impl DkimResolver for OneShotResolver {
    async fn lookup_txt(&self, _q: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
        Ok(vec![self.txt.clone()])
    }
}

#[tokio::test]
async fn sign_and_verify_rsa_sha256_roundtrip() {
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let key = DkimSigningKey::Rsa(priv_key);

    let body = b"Hello, world.\r\n";
    let msg = {
        let mut v = Vec::new();
        v.extend_from_slice(b"From: alice@example.com\r\n");
        v.extend_from_slice(b"To: bob@example.com\r\n");
        v.extend_from_slice(b"Subject: roundtrip\r\n");
        v.extend_from_slice(b"Date: Mon, 23 May 2026 00:00:00 +0000\r\n");
        v.extend_from_slice(b"\r\n");
        v.extend_from_slice(body);
        v
    };

    let opts = SignOpts::new("example.com", "s1").signed_headers(["From", "To", "Subject", "Date"]);
    let header_line = sign(&msg, &key, &opts).unwrap();
    assert!(header_line.starts_with("DKIM-Signature: "));
    assert!(header_line.ends_with("\r\n"));
    assert!(header_line.contains("a=rsa-sha256"));
    assert!(header_line.contains("c=relaxed/relaxed"));
    assert!(header_line.contains("d=example.com"));
    assert!(header_line.contains("s=s1"));

    // Prepend to the message and verify.
    let mut signed_msg = Vec::with_capacity(msg.len() + header_line.len());
    signed_msg.extend_from_slice(header_line.as_bytes());
    signed_msg.extend_from_slice(&msg);

    let resolver = OneShotResolver {
        txt: rsa_pubkey_txt(&pub_key),
    };
    let outputs = verify_all(&resolver, &signed_msg).await;
    assert_eq!(outputs.len(), 1, "should find exactly one DKIM-Signature");
    assert!(
        matches!(outputs[0].result, DkimResult::Pass),
        "RSA-SHA256 sign→verify failed: {:?}",
        outputs[0].result
    );
}

#[tokio::test]
async fn sign_and_verify_ed25519_sha256_roundtrip() {
    let mut rng = rand::thread_rng();
    use rand::RngCore as _;
    let mut secret = [0u8; 32];
    rng.fill_bytes(&mut secret);
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();
    let key = DkimSigningKey::Ed25519(signing_key);

    let msg = {
        let mut v = Vec::new();
        v.extend_from_slice(b"From: alice@example.com\r\n");
        v.extend_from_slice(b"Subject: ed25519 roundtrip\r\n");
        v.extend_from_slice(b"\r\n");
        v.extend_from_slice(b"body\r\n");
        v
    };

    let opts = SignOpts::new("example.com", "ed1").signed_headers(["From", "Subject"]);
    let header_line = sign(&msg, &key, &opts).unwrap();
    assert!(header_line.contains("a=ed25519-sha256"));

    let mut signed_msg = Vec::new();
    signed_msg.extend_from_slice(header_line.as_bytes());
    signed_msg.extend_from_slice(&msg);

    let resolver = OneShotResolver {
        txt: ed25519_pubkey_txt(&verifying_key),
    };
    let outputs = verify_all(&resolver, &signed_msg).await;
    assert_eq!(outputs.len(), 1);
    assert!(
        matches!(outputs[0].result, DkimResult::Pass),
        "Ed25519-SHA256 sign→verify failed: {:?}",
        outputs[0].result
    );
}

#[tokio::test]
async fn sign_with_optional_tags_round_trips() {
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let key = DkimSigningKey::Rsa(priv_key);

    let msg = {
        let mut v = Vec::new();
        v.extend_from_slice(b"From: alice@example.com\r\n");
        v.extend_from_slice(b"\r\n");
        v.extend_from_slice(b"body\r\n");
        v
    };
    // x= must be in the future so verify doesn't reject as Expired.
    // 2099-01-01 epoch = 4_070_908_800.
    let opts = SignOpts::new("example.com", "s1")
        .signed_headers(["From"])
        .timestamp(1_700_000_000)
        .expiration(4_070_908_800);
    let header_line = sign(&msg, &key, &opts).unwrap();
    assert!(header_line.contains("t=1700000000"));
    assert!(header_line.contains("x=4070908800"));

    let mut signed_msg = Vec::new();
    signed_msg.extend_from_slice(header_line.as_bytes());
    signed_msg.extend_from_slice(&msg);
    let resolver = OneShotResolver {
        txt: rsa_pubkey_txt(&pub_key),
    };
    let outputs = verify_all(&resolver, &signed_msg).await;
    assert!(matches!(outputs[0].result, DkimResult::Pass));
}

#[tokio::test]
async fn sign_simple_canon_round_trips() {
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let key = DkimSigningKey::Rsa(priv_key);

    let msg = b"From: alice@example.com\r\nSubject: t\r\n\r\nbody\r\n".to_vec();
    let mut opts = SignOpts::new("example.com", "s1").signed_headers(["From", "Subject"]);
    opts.canon_header = Canon::Simple;
    opts.canon_body = Canon::Simple;
    let header_line = sign(&msg, &key, &opts).unwrap();
    assert!(header_line.contains("c=simple/simple"));

    let mut signed_msg = Vec::new();
    signed_msg.extend_from_slice(header_line.as_bytes());
    signed_msg.extend_from_slice(&msg);
    let resolver = OneShotResolver {
        txt: rsa_pubkey_txt(&pub_key),
    };
    let outputs = verify_all(&resolver, &signed_msg).await;
    assert!(
        matches!(outputs[0].result, DkimResult::Pass),
        "simple/simple sign→verify failed: {:?}",
        outputs[0].result
    );
}
