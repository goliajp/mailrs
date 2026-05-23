//! End-to-end multi-signature DKIM verification.
//!
//! Build a message with TWO real DKIM-Signature headers, signed by
//! TWO different keypairs on TWO different selectors, then call
//! `verify_all` with a resolver that returns the right public key
//! per query. Both signatures must verify → `Vec<SignatureOutput>`
//! with two `Pass` entries carrying the expected `d=`.

use base64::Engine as _;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;

use mailrs_dkim::canon::{canonicalize_body, canonicalize_header};
use mailrs_dkim::header::Canon as DkimCanon;
use mailrs_dkim::headers::{
    body_offset_minus_blank, clear_b_value, find_body_offset, find_header_value,
};
use mailrs_dkim::{DkimResolver, DkimResult, verify_all};

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn pubkey_txt(public_key: &RsaPublicKey) -> String {
    let der = public_key.to_public_key_der().unwrap();
    format!("v=DKIM1; k=rsa; p={}", b64(der.as_bytes()))
}

/// Resolver that maps each `<sel>._domainkey.<domain>` query to a
/// pre-computed TXT record.
struct MapResolver {
    map: std::collections::HashMap<String, String>,
}

#[async_trait::async_trait]
impl DkimResolver for MapResolver {
    async fn lookup_txt(&self, q: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
        Ok(self.map.get(q).cloned().map(|v| vec![v]).unwrap_or_default())
    }
}

/// Helper: produce a DKIM-Signature header value (with `b=` filled in)
/// for a given message body + signed-headers + key.
///
/// Builds an unsigned AMS-like row first, then computes the signed
/// block exactly as `verify` would, then signs and fills `b=`.
fn sign_dkim(
    body_hash_b64: &str,
    domain: &str,
    selector: &str,
    pre_signing_msg: &[u8],
    signing_key: &SigningKey<Sha256>,
) -> String {
    let unsigned = format!(
        "DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d={domain}; s={selector}; \
         h=From:To:Subject; bh={body_hash_b64}; b=\r\n"
    );
    // Build a temporary message header region that includes the
    // unsigned DKIM-Signature so find_header_value can pull its value.
    let mut tmp = Vec::new();
    tmp.extend_from_slice(unsigned.as_bytes());
    tmp.extend_from_slice(pre_signing_msg);
    let body_offset = find_body_offset(&tmp).unwrap();
    let headers_region = &tmp[..body_offset_minus_blank(body_offset, &tmp)];
    let mut signed = Vec::new();
    for name in ["From", "To", "Subject"] {
        let v = find_header_value(headers_region, name).unwrap();
        signed.extend_from_slice(&canonicalize_header(name, v, DkimCanon::Relaxed));
    }
    let dkim_value = find_header_value(headers_region, "DKIM-Signature").unwrap();
    let cleared = clear_b_value(dkim_value);
    let canon_dkim = canonicalize_header("DKIM-Signature", &cleared, DkimCanon::Relaxed);
    let canon_dkim_trimmed = if canon_dkim.ends_with(b"\r\n") {
        &canon_dkim[..canon_dkim.len() - 2]
    } else {
        &canon_dkim
    };
    signed.extend_from_slice(canon_dkim_trimmed);
    let sig = signing_key.sign(&signed);
    let sig_b64 = b64(&sig.to_bytes());
    format!(
        "DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed; d={domain}; s={selector}; \
         h=From:To:Subject; bh={body_hash_b64}; b={sig_b64}\r\n"
    )
}

#[tokio::test]
async fn verify_all_two_signatures_both_pass() {
    let mut rng = rand::thread_rng();
    let key_a = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_a = RsaPublicKey::from(&key_a);
    let sign_a = SigningKey::<Sha256>::new(key_a);

    let key_b = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_b = RsaPublicKey::from(&key_b);
    let sign_b = SigningKey::<Sha256>::new(key_b);

    let body = b"Multi-sig test.\r\n";
    let canon_body = canonicalize_body(body, DkimCanon::Relaxed, None);
    let mut h = <Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, &canon_body);
    let body_hash = b64(sha2::Digest::finalize(h).as_slice());

    // The "rest of the message" the DKIM signing helper appends after
    // the unsigned DKIM-Signature row: From, To, Subject, body.
    let pre_signing = b"From: alice@a.com\r\nTo: bob@example.com\r\nSubject: hi\r\n\r\nMulti-sig test.\r\n";

    let sig_a = sign_dkim(&body_hash, "a.com", "selA", pre_signing, &sign_a);
    let sig_b = sign_dkim(&body_hash, "b.com", "selB", pre_signing, &sign_b);

    // Final message order: sig_a, sig_b, From, To, Subject, blank, body
    let mut msg = Vec::new();
    msg.extend_from_slice(sig_a.as_bytes());
    msg.extend_from_slice(sig_b.as_bytes());
    msg.extend_from_slice(pre_signing);

    let mut map = std::collections::HashMap::new();
    map.insert("selA._domainkey.a.com".into(), pubkey_txt(&pub_a));
    map.insert("selB._domainkey.b.com".into(), pubkey_txt(&pub_b));
    let resolver = MapResolver { map };

    let outputs = verify_all(&resolver, &msg).await;
    assert_eq!(outputs.len(), 2, "should find 2 signatures, got {outputs:?}");
    for (i, out) in outputs.iter().enumerate() {
        assert!(
            matches!(out.result, DkimResult::Pass),
            "signature {i} did not pass: {:?}",
            out.result
        );
    }
    let domains: Vec<_> = outputs.iter().map(|o| o.domain().to_string()).collect();
    assert!(domains.contains(&"a.com".to_string()));
    assert!(domains.contains(&"b.com".to_string()));
}

#[tokio::test]
async fn verify_all_returns_empty_when_no_dkim_signature() {
    let msg = b"From: a@b\r\n\r\nbody";
    let resolver = MapResolver {
        map: std::collections::HashMap::new(),
    };
    let outputs = verify_all(&resolver, msg).await;
    assert!(outputs.is_empty());
}

#[tokio::test]
async fn verify_all_one_pass_one_fail() {
    // Two signatures: first one signed correctly, second one tampered
    // (we mutate its b=). verify_all must return both, with Pass + Fail.
    let mut rng = rand::thread_rng();
    let key_a = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_a = RsaPublicKey::from(&key_a);
    let sign_a = SigningKey::<Sha256>::new(key_a);

    let body = b"One pass one fail.\r\n";
    let canon_body = canonicalize_body(body, DkimCanon::Relaxed, None);
    let mut h = <Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, &canon_body);
    let body_hash = b64(sha2::Digest::finalize(h).as_slice());

    let pre = b"From: a@a.com\r\nTo: b@b.com\r\nSubject: t\r\n\r\nOne pass one fail.\r\n";
    let sig_good = sign_dkim(&body_hash, "a.com", "sel1", pre, &sign_a);
    // Build a second signature claiming d=b.com but signed with key_a's
    // private key (so resolver returns pub_a but b.com's d= doesn't
    // align — verify will look up sel2._domainkey.b.com which returns
    // pub_a's TXT in our map, but the signing covers d=b.com so the
    // canon_dkim differs from what verify reconstructs → SignatureMismatch).
    // Easier: just mutate `b=` in sig_good to break it.
    let sig_bad = sig_good.replace("b=", "b=XYZ");

    let mut msg = Vec::new();
    msg.extend_from_slice(sig_good.as_bytes());
    msg.extend_from_slice(sig_bad.as_bytes());
    msg.extend_from_slice(pre);

    let mut map = std::collections::HashMap::new();
    map.insert("sel1._domainkey.a.com".into(), pubkey_txt(&pub_a));
    let resolver = MapResolver { map };
    let outputs = verify_all(&resolver, &msg).await;
    assert_eq!(outputs.len(), 2);
    let passes = outputs.iter().filter(|o| o.is_pass()).count();
    assert_eq!(passes, 1, "expected exactly one Pass, got: {outputs:?}");
}
