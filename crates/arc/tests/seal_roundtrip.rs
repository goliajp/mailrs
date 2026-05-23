//! End-to-end ARC sealing → verifying roundtrip.
//!
//! For each scenario (first hop, second hop on top of an existing
//! 1-hop chain), generate a real RSA-2048 keypair, [`seal`] the
//! message, prepend the three headers, then run
//! [`verify_chain_with_crypto`] against a resolver that returns the
//! matching public key TXT. The chain MUST verify Pass — if seal
//! and verify ever disagree, every chain this crate produces is
//! unusable.

use base64::Engine as _;
use rsa::pkcs8::EncodePublicKey;
use rsa::{RsaPrivateKey, RsaPublicKey};

use mailrs_arc::{
    ArcChain, ArcSealCv, ArcSigningKey, Canon, ChainOutcome, SealOpts, seal,
    verify_chain_with_crypto,
};

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn pubkey_txt(public_key: &RsaPublicKey) -> String {
    let der = public_key.to_public_key_der().unwrap();
    format!("v=DKIM1; k=rsa; p={}", b64(der.as_bytes()))
}

struct OneShotResolver {
    txt: String,
}

#[async_trait::async_trait]
impl mailrs_dkim::DkimResolver for OneShotResolver {
    async fn lookup_txt(&self, _q: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
        Ok(vec![self.txt.clone()])
    }
}

#[tokio::test]
async fn seal_first_hop_and_verify_passes() {
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);
    let key = ArcSigningKey::Rsa(&priv_key);

    let raw_msg = b"From: alice@origin.example\r\nTo: bob@forwarder.example\r\nSubject: t\r\n\r\nbody\r\n".to_vec();

    let opts = SealOpts {
        domain: "forwarder.example".into(),
        selector: "s1".into(),
        signed_headers: vec!["From".into(), "To".into(), "Subject".into()],
        canon_header: Canon::Relaxed,
        canon_body: Canon::Relaxed,
        cv: ArcSealCv::None,
        authres: "spf=pass smtp.mailfrom=alice@origin.example; dkim=pass".into(),
        timestamp: Some(1_700_000_000),
    };

    let sealed = seal(&raw_msg, &key, &opts, None).unwrap();
    assert!(sealed.aar.contains("i=1;"));
    assert!(sealed.ams.contains("i=1;"));
    assert!(sealed.seal.contains("i=1;"));
    assert!(sealed.seal.contains("cv=none"));

    // Prepend the three headers (AAR, AMS, AS order) to the message.
    let mut final_msg = Vec::new();
    final_msg.extend_from_slice(sealed.concat().as_bytes());
    final_msg.extend_from_slice(&raw_msg);

    let chain = ArcChain::extract(&final_msg).unwrap().unwrap();
    assert_eq!(chain.sets.len(), 1);
    let resolver = OneShotResolver {
        txt: pubkey_txt(&pub_key),
    };
    let outcome = verify_chain_with_crypto(&chain, &resolver, &final_msg)
        .await
        .unwrap();
    match outcome {
        ChainOutcome::Pass => {}
        other => panic!("expected Pass for first-hop seal, got: {other:?}"),
    }
}

#[tokio::test]
async fn seal_second_hop_on_existing_chain_verifies() {
    // Build a first-hop chain with key_a, then a second-hop seal
    // with key_b. verify_chain_with_crypto needs to look up BOTH
    // keys, so the resolver maps each `<selector>._domainkey.<domain>`
    // to the right TXT.
    let mut rng = rand::thread_rng();
    let priv_a = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_a = RsaPublicKey::from(&priv_a);
    let key_a = ArcSigningKey::Rsa(&priv_a);

    let priv_b = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pub_b = RsaPublicKey::from(&priv_b);
    let key_b = ArcSigningKey::Rsa(&priv_b);

    let raw_msg =
        b"From: alice@origin.example\r\nTo: bob@b.example\r\nSubject: chain\r\n\r\nbody\r\n"
            .to_vec();

    // First seal: i=1, cv=none.
    let opts1 = SealOpts {
        domain: "a.example".into(),
        selector: "sela".into(),
        signed_headers: vec!["From".into(), "To".into(), "Subject".into()],
        canon_header: Canon::Relaxed,
        canon_body: Canon::Relaxed,
        cv: ArcSealCv::None,
        authres: "spf=pass; dkim=pass".into(),
        timestamp: Some(1_700_000_000),
    };
    let sealed1 = seal(&raw_msg, &key_a, &opts1, None).unwrap();
    let mut after_hop1 = Vec::new();
    after_hop1.extend_from_slice(sealed1.concat().as_bytes());
    after_hop1.extend_from_slice(&raw_msg);

    let chain_after_1 = ArcChain::extract(&after_hop1).unwrap().unwrap();
    assert_eq!(chain_after_1.sets.len(), 1);

    // Second seal: i=2, cv=pass (because the first hop's signature
    // is genuine in this test setup).
    let opts2 = SealOpts {
        domain: "b.example".into(),
        selector: "selb".into(),
        signed_headers: vec!["From".into(), "To".into(), "Subject".into()],
        canon_header: Canon::Relaxed,
        canon_body: Canon::Relaxed,
        cv: ArcSealCv::Pass,
        authres: "spf=pass; dkim=pass; arc=pass".into(),
        timestamp: Some(1_700_000_100),
    };
    let sealed2 = seal(&after_hop1, &key_b, &opts2, Some(&chain_after_1)).unwrap();
    assert!(sealed2.aar.contains("i=2;"));
    assert!(sealed2.seal.contains("cv=pass"));

    let mut after_hop2 = Vec::new();
    after_hop2.extend_from_slice(sealed2.concat().as_bytes());
    after_hop2.extend_from_slice(&after_hop1);

    let chain_after_2 = ArcChain::extract(&after_hop2).unwrap().unwrap();
    assert_eq!(chain_after_2.sets.len(), 2);

    // Resolver returns the right key per query.
    let txt_a = pubkey_txt(&pub_a);
    let txt_b = pubkey_txt(&pub_b);
    struct TwoKeyResolver {
        txt_a: String,
        txt_b: String,
    }
    #[async_trait::async_trait]
    impl mailrs_dkim::DkimResolver for TwoKeyResolver {
        async fn lookup_txt(&self, q: &str) -> Result<Vec<String>, mailrs_dkim::DkimError> {
            if q.contains("a.example") {
                Ok(vec![self.txt_a.clone()])
            } else if q.contains("b.example") {
                Ok(vec![self.txt_b.clone()])
            } else {
                Ok(vec![])
            }
        }
    }
    let resolver = TwoKeyResolver { txt_a, txt_b };
    let outcome = verify_chain_with_crypto(&chain_after_2, &resolver, &after_hop2)
        .await
        .unwrap();
    match outcome {
        ChainOutcome::Pass => {}
        other => panic!("expected Pass for 2-hop seal, got: {other:?}"),
    }
}

#[tokio::test]
async fn seal_rejects_first_hop_with_non_none_cv() {
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let key = ArcSigningKey::Rsa(&priv_key);
    let raw_msg = b"From: a@b\r\n\r\nbody\r\n".to_vec();

    let opts = SealOpts {
        domain: "x.example".into(),
        selector: "s".into(),
        signed_headers: vec!["From".into()],
        canon_header: Canon::Relaxed,
        canon_body: Canon::Relaxed,
        cv: ArcSealCv::Pass, // illegal: no prior chain
        authres: "spf=pass".into(),
        timestamp: None,
    };
    let r = seal(&raw_msg, &key, &opts, None);
    assert!(matches!(r, Err(mailrs_arc::ArcError::InvalidCv(_))));
}

#[tokio::test]
async fn seal_rejects_later_hop_with_cv_none() {
    let mut rng = rand::thread_rng();
    let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let key = ArcSigningKey::Rsa(&priv_key);

    // Seal a first hop to give us a "prior chain", then try to seal
    // a second hop with cv=none — which should be rejected.
    let raw_msg = b"From: a@b\r\nSubject: t\r\n\r\nbody\r\n".to_vec();
    let opts1 = SealOpts {
        domain: "a.example".into(),
        selector: "s".into(),
        signed_headers: vec!["From".into(), "Subject".into()],
        canon_header: Canon::Relaxed,
        canon_body: Canon::Relaxed,
        cv: ArcSealCv::None,
        authres: "spf=pass".into(),
        timestamp: None,
    };
    let sealed1 = seal(&raw_msg, &key, &opts1, None).unwrap();
    let mut after_hop1 = Vec::new();
    after_hop1.extend_from_slice(sealed1.concat().as_bytes());
    after_hop1.extend_from_slice(&raw_msg);
    let chain = ArcChain::extract(&after_hop1).unwrap().unwrap();

    let opts2 = SealOpts {
        cv: ArcSealCv::None, // illegal: prior chain exists
        ..opts1.clone()
    };
    let r = seal(&after_hop1, &key, &opts2, Some(&chain));
    assert!(matches!(r, Err(mailrs_arc::ArcError::InvalidCv(_))));
}
