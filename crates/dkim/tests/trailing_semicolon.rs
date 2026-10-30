//! A signature whose tag list ends in `;` after `b=` (LinkedIn's does) is
//! as valid as one that stops at `b=`. The header is signed here by hand,
//! with the `rsa` crate, so the test does not lean on this crate's signer.

use base64::Engine as _;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};

use mailrs_dkim::{Canon, DkimResolver, DkimResult, canon, verify_all};

struct OneKey(String);

#[async_trait::async_trait]
impl DkimResolver for OneKey {
    async fn lookup_txt(&self, _q: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
        Ok(vec![self.0.clone()])
    }
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// A message signed over From and Subject, its DKIM-Signature ending in
/// `b=<sig>;` and folded across lines the way large senders fold it.
fn signed(key: &RsaPrivateKey, c: &str, header_canon: Canon) -> Vec<u8> {
    let from = " Alice <alice@example.com>";
    let subject = " a trailing semicolon";
    let body = b"Hello.\r\n";
    let bh = b64(&Sha256::digest(canon::canonicalize_body(
        body,
        Canon::Relaxed,
        None,
    )));
    let tags = |b: &str| {
        format!(
            " v=1; a=rsa-sha256; c={c}; d=example.com; s=s1;\r\n\th=from:subject; bh={bh};\r\n\tb={b};"
        )
    };
    let mut input = Vec::new();
    input.extend(canon::canonicalize_header("From", from, header_canon));
    input.extend(canon::canonicalize_header("Subject", subject, header_canon));
    let own = canon::canonicalize_header("DKIM-Signature", &tags(""), header_canon);
    input.extend_from_slice(own.strip_suffix(b"\r\n").unwrap_or(&own));
    let sig = SigningKey::<Sha256>::new(key.clone()).sign(&input).to_vec();
    let mut msg = format!(
        "DKIM-Signature:{}\r\nFrom:{from}\r\nSubject:{subject}\r\n\r\n",
        tags(&b64(&sig))
    )
    .into_bytes();
    msg.extend_from_slice(body);
    msg
}

#[tokio::test]
async fn a_semicolon_after_b_still_verifies() {
    let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let der = RsaPublicKey::from(&key).to_public_key_der().unwrap();
    let resolver = OneKey(format!("v=DKIM1; k=rsa; p={}", b64(der.as_bytes())));
    for (c, header_canon) in [
        ("relaxed/relaxed", Canon::Relaxed),
        ("simple/relaxed", Canon::Simple),
    ] {
        let msg = signed(&key, c, header_canon);
        let out = verify_all(&resolver, &msg).await;
        assert_eq!(out.len(), 1);
        assert!(
            matches!(out[0].result, DkimResult::Pass),
            "{c}: {:?}",
            out[0].result
        );
    }
}
