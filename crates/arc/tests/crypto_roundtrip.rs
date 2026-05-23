//! End-to-end crypto roundtrip for mailrs-arc 1.1.
//!
//! Generate a real RSA-2048 keypair, build a 1-hop ARC chain (AAR +
//! AMS + AS), sign the AMS over a real message + the AS over the
//! canonicalized chain prefix, then run `verify_chain_with_crypto`
//! end-to-end with a `DummyResolver` returning the matching public
//! key TXT. Both signatures must validate → `ChainOutcome::Pass`.
//!
//! This is the test that proves the crypto path actually works
//! against itself — there is no "trust me" intermediate.

use base64::Engine as _;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::EncodePublicKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;

use mailrs_arc::{ArcChain, ChainOutcome, verify_chain_with_crypto};
use mailrs_dkim::canon::{canonicalize_body, canonicalize_header};
use mailrs_dkim::header::Canon as DkimCanon;
use mailrs_dkim::headers::{
    body_offset_minus_blank, clear_b_value, find_body_offset, find_header_value,
};

const DOMAIN: &str = "example.com";
const SELECTOR: &str = "test";

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// Build the public-key TXT a DkimResolver would return.
fn public_key_txt(public_key: &RsaPublicKey) -> String {
    // The `p=` payload is the PKCS8 SubjectPublicKeyInfo DER in base64.
    let der = public_key.to_public_key_der().unwrap();
    let p_b64 = b64(der.as_bytes());
    format!("v=DKIM1; k=rsa; p={p_b64}")
}

struct DummyResolver {
    txt: String,
}

#[async_trait::async_trait]
impl mailrs_dkim::DkimResolver for DummyResolver {
    async fn lookup_txt(&self, _: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
        Ok(vec![self.txt.clone()])
    }
}

#[tokio::test]
async fn one_hop_chain_rsa_sha256_roundtrip_passes() {
    // 1. Keypair.
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let signing_key = SigningKey::<Sha256>::new(priv_key.clone());

    // 2. Body.
    let body = b"Hello ARC world.\r\n";
    let canon_body = canonicalize_body(body, DkimCanon::Relaxed, None);
    let mut h = <Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, &canon_body);
    let body_hash = b64(sha2::Digest::finalize(h).as_slice());

    // 3. Construct the message with placeholder AMS + AS (b= empty).
    // We need real From/To/Subject because h= lists them.
    let from = "From: alice@example.com\r\n";
    let to = "To: bob@forwarder.example\r\n";
    let subject = "Subject: roundtrip\r\n";

    let aar = "ARC-Authentication-Results: i=1; spf=pass smtp.mailfrom=alice@example.com\r\n";
    // AMS with empty b= (signature TBD)
    let ams_no_sig = format!(
        "ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d={DOMAIN}; \
         s={SELECTOR}; h=From:To:Subject; bh={body_hash}; b=\r\n"
    );

    // Build the header block for signed-headers + AMS canon
    let headers_for_signing = {
        let mut v = Vec::new();
        v.extend_from_slice(aar.as_bytes());
        v.extend_from_slice(ams_no_sig.as_bytes());
        v.extend_from_slice(from.as_bytes());
        v.extend_from_slice(to.as_bytes());
        v.extend_from_slice(subject.as_bytes());
        // Trailing CRLF + body
        v.extend_from_slice(b"\r\n");
        v.extend_from_slice(body);
        v
    };

    // 4. Compute the AMS signed block exactly as the verifier would.
    let body_offset = find_body_offset(&headers_for_signing).unwrap();
    let headers_region = &headers_for_signing[..body_offset_minus_blank(body_offset, &headers_for_signing)];

    let mut signed_block = Vec::new();
    for h_name in ["from", "to", "subject"] {
        let value = find_header_value(headers_region, h_name).unwrap();
        signed_block.extend_from_slice(&canonicalize_header(h_name, value, DkimCanon::Relaxed));
    }
    // Extract the AMS header value (everything after `ARC-Message-Signature:`)
    let ams_value = find_header_value(headers_region, "ARC-Message-Signature").unwrap();
    let ams_cleared = clear_b_value(ams_value);
    let canon_ams = canonicalize_header("ARC-Message-Signature", &ams_cleared, DkimCanon::Relaxed);
    let canon_ams_trimmed = if canon_ams.ends_with(b"\r\n") {
        &canon_ams[..canon_ams.len() - 2]
    } else {
        &canon_ams
    };
    signed_block.extend_from_slice(canon_ams_trimmed);

    // 5. Sign the AMS block.
    let ams_sig = signing_key.sign(&signed_block);
    let ams_sig_b64 = b64(&ams_sig.to_bytes());
    let ams_signed = format!(
        "ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d={DOMAIN}; \
         s={SELECTOR}; h=From:To:Subject; bh={body_hash}; b={ams_sig_b64}\r\n"
    );

    // 6. Build the AS signed block (relaxed canon of AAR + AMS + AS-with-b=empty).
    // AS value extracted from a placeholder AS header.
    let as_no_sig = format!(
        "ARC-Seal: i=1; a=rsa-sha256; cv=none; d={DOMAIN}; s={SELECTOR}; b=\r\n"
    );
    // For canonicalizing AAR / AMS we want their values without the `Name:` prefix.
    let aar_value = aar.trim_start_matches("ARC-Authentication-Results:");
    let aar_value = aar_value.trim_start_matches(' ').trim_end_matches("\r\n");
    let ams_value = ams_signed.trim_start_matches("ARC-Message-Signature:");
    let ams_value = ams_value.trim_start_matches(' ').trim_end_matches("\r\n");
    let as_value = as_no_sig.trim_start_matches("ARC-Seal:");
    let as_value = as_value.trim_start_matches(' ').trim_end_matches("\r\n");
    let as_cleared = clear_b_value(as_value);

    let mut as_signed_block = Vec::new();
    as_signed_block.extend_from_slice(&canonicalize_header(
        "ARC-Authentication-Results",
        aar_value,
        DkimCanon::Relaxed,
    ));
    as_signed_block.extend_from_slice(&canonicalize_header(
        "ARC-Message-Signature",
        ams_value,
        DkimCanon::Relaxed,
    ));
    let canon_as = canonicalize_header("ARC-Seal", &as_cleared, DkimCanon::Relaxed);
    let canon_as_trimmed = if canon_as.ends_with(b"\r\n") {
        &canon_as[..canon_as.len() - 2]
    } else {
        &canon_as
    };
    as_signed_block.extend_from_slice(canon_as_trimmed);

    let as_sig = signing_key.sign(&as_signed_block);
    let as_sig_b64 = b64(&as_sig.to_bytes());
    let as_signed = format!(
        "ARC-Seal: i=1; a=rsa-sha256; cv=none; d={DOMAIN}; s={SELECTOR}; b={as_sig_b64}\r\n"
    );

    // 7. Assemble the final message.
    let mut final_msg = Vec::new();
    final_msg.extend_from_slice(aar.as_bytes());
    final_msg.extend_from_slice(ams_signed.as_bytes());
    final_msg.extend_from_slice(as_signed.as_bytes());
    final_msg.extend_from_slice(from.as_bytes());
    final_msg.extend_from_slice(to.as_bytes());
    final_msg.extend_from_slice(subject.as_bytes());
    final_msg.extend_from_slice(b"\r\n");
    final_msg.extend_from_slice(body);

    // 8. Extract chain + verify with crypto.
    let chain = ArcChain::extract(&final_msg).unwrap().unwrap();
    assert_eq!(chain.sets.len(), 1);

    let resolver = DummyResolver {
        txt: public_key_txt(&pub_key),
    };
    let outcome = verify_chain_with_crypto(&chain, &resolver, &final_msg)
        .await
        .unwrap();
    match outcome {
        ChainOutcome::Pass => {} // expected
        other => panic!("expected Pass, got: {other:?}"),
    }
}

#[tokio::test]
async fn tampered_body_makes_body_hash_mismatch_fail() {
    // Generate, sign, then mutate the body — verify must fail.
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let signing_key = SigningKey::<Sha256>::new(priv_key.clone());

    let body = b"Original body.\r\n";
    let canon_body = canonicalize_body(body, DkimCanon::Relaxed, None);
    let mut h = <Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, &canon_body);
    let body_hash = b64(sha2::Digest::finalize(h).as_slice());

    let from = "From: alice@example.com\r\n";
    let aar = "ARC-Authentication-Results: i=1; spf=pass\r\n";
    let ams_no_sig = format!(
        "ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d={DOMAIN}; \
         s={SELECTOR}; h=From; bh={body_hash}; b=\r\n"
    );

    let pre = {
        let mut v = Vec::new();
        v.extend_from_slice(aar.as_bytes());
        v.extend_from_slice(ams_no_sig.as_bytes());
        v.extend_from_slice(from.as_bytes());
        v.extend_from_slice(b"\r\n");
        v.extend_from_slice(body);
        v
    };
    let body_offset = find_body_offset(&pre).unwrap();
    let headers_region = &pre[..body_offset_minus_blank(body_offset, &pre)];
    let mut signed_block = Vec::new();
    let v = find_header_value(headers_region, "From").unwrap();
    signed_block.extend_from_slice(&canonicalize_header("from", v, DkimCanon::Relaxed));
    let ams_value = find_header_value(headers_region, "ARC-Message-Signature").unwrap();
    let ams_cleared = clear_b_value(ams_value);
    let canon_ams = canonicalize_header("ARC-Message-Signature", &ams_cleared, DkimCanon::Relaxed);
    let canon_ams_trimmed = if canon_ams.ends_with(b"\r\n") {
        &canon_ams[..canon_ams.len() - 2]
    } else {
        &canon_ams
    };
    signed_block.extend_from_slice(canon_ams_trimmed);
    let ams_sig = signing_key.sign(&signed_block);
    let ams_sig_b64 = b64(&ams_sig.to_bytes());
    let ams_signed = format!(
        "ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d={DOMAIN}; \
         s={SELECTOR}; h=From; bh={body_hash}; b={ams_sig_b64}\r\n"
    );

    // AS (minimal — uses similar logic but we only care about AMS path here)
    let as_no_sig = format!("ARC-Seal: i=1; a=rsa-sha256; cv=none; d={DOMAIN}; s={SELECTOR}; b=\r\n");
    let aar_v = aar.trim_start_matches("ARC-Authentication-Results:")
        .trim_start_matches(' ')
        .trim_end_matches("\r\n");
    let ams_v = ams_signed.trim_start_matches("ARC-Message-Signature:")
        .trim_start_matches(' ')
        .trim_end_matches("\r\n");
    let as_v = as_no_sig.trim_start_matches("ARC-Seal:")
        .trim_start_matches(' ')
        .trim_end_matches("\r\n");
    let as_cleared = clear_b_value(as_v);
    let mut as_sb = Vec::new();
    as_sb.extend_from_slice(&canonicalize_header(
        "ARC-Authentication-Results",
        aar_v,
        DkimCanon::Relaxed,
    ));
    as_sb.extend_from_slice(&canonicalize_header(
        "ARC-Message-Signature",
        ams_v,
        DkimCanon::Relaxed,
    ));
    let canon_as = canonicalize_header("ARC-Seal", &as_cleared, DkimCanon::Relaxed);
    let canon_as_t = if canon_as.ends_with(b"\r\n") {
        &canon_as[..canon_as.len() - 2]
    } else {
        &canon_as
    };
    as_sb.extend_from_slice(canon_as_t);
    let as_sig = signing_key.sign(&as_sb);
    let as_sig_b64 = b64(&as_sig.to_bytes());
    let as_signed = format!(
        "ARC-Seal: i=1; a=rsa-sha256; cv=none; d={DOMAIN}; s={SELECTOR}; b={as_sig_b64}\r\n"
    );

    // Final message — TAMPER the body
    let tampered_body = b"Tampered body!!\r\n";
    let mut final_msg = Vec::new();
    final_msg.extend_from_slice(aar.as_bytes());
    final_msg.extend_from_slice(ams_signed.as_bytes());
    final_msg.extend_from_slice(as_signed.as_bytes());
    final_msg.extend_from_slice(from.as_bytes());
    final_msg.extend_from_slice(b"\r\n");
    final_msg.extend_from_slice(tampered_body);

    let chain = ArcChain::extract(&final_msg).unwrap().unwrap();
    let resolver = DummyResolver {
        txt: public_key_txt(&pub_key),
    };
    let outcome = verify_chain_with_crypto(&chain, &resolver, &final_msg)
        .await
        .unwrap();
    match outcome {
        ChainOutcome::Fail { reason } => {
            assert!(
                reason.contains("body hash") || reason.contains("ams i=1"),
                "expected body-hash failure, got: {reason}"
            );
        }
        other => panic!("expected Fail (body hash mismatch), got: {other:?}"),
    }
}
