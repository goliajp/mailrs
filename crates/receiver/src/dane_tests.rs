use super::*;
use hickory_resolver::proto::op::{MessageType, OpCode};
use hickory_resolver::proto::rr::Record;
use hickory_resolver::proto::rr::rdata::TLSA;
use hickory_resolver::proto::rr::rdata::tlsa::{CertUsage, Matching, Selector};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cert(key: &rcgen::KeyPair) -> String {
    rcgen::CertificateParams::new(vec!["mx.example".to_string()])
        .unwrap()
        .self_signed(key)
        .unwrap()
        .pem()
}

/// The record is the SHA-256 of the key's SubjectPublicKeyInfo, the same
/// bytes `openssl x509 -pubkey | openssl pkey -pubin -outform DER` hashes.
#[test]
fn a_certificate_and_its_public_key_give_the_same_record() {
    let key = rcgen::KeyPair::generate().unwrap();
    let want = format!("3 1 1 {}", hex(&Sha256::digest(key.public_key_der())));
    assert_eq!(
        record_for_cert_pem(cert(&key).as_bytes()),
        Some(want.clone())
    );
    assert_eq!(
        record_for_pubkey_pem(key.public_key_pem().as_bytes()),
        Some(want)
    );
}

#[test]
fn the_record_is_taken_from_the_leaf_of_a_chain() {
    let leaf = rcgen::KeyPair::generate().unwrap();
    let other = rcgen::KeyPair::generate().unwrap();
    let chain = format!("{}{}", cert(&leaf), cert(&other));
    assert_eq!(
        record_for_cert_pem(chain.as_bytes()),
        record_for_pubkey_pem(leaf.public_key_pem().as_bytes())
    );
    assert_eq!(record_for_cert_pem(b"not a certificate"), None);
}

fn config(dir: &std::path::Path, next: bool) -> DaneConfig {
    DaneConfig {
        hostname: "mx.example".into(),
        cert: dir.join("fullchain.pem"),
        next_key: next.then(|| dir.join("next-pubkey.pem")),
        resolvers: vec![],
        authoritative: vec![],
    }
}

fn published(cell: &DaneCell, records: &[&String]) {
    let records: Vec<String> = records.iter().map(|r| r.to_string()).collect();
    cell.update(|s| {
        s.sources = ["1.1.1.1", "8.8.8.8"]
            .iter()
            .map(|r| DaneSource {
                server: r.to_string(),
                authenticated: Some(true),
                records: records.clone(),
                error: None,
            })
            .collect();
    });
}

#[test]
fn a_swap_to_the_published_next_key_is_not_counted_and_one_to_an_unknown_key_is() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), true);
    let (k1, k2, k3) = (
        rcgen::KeyPair::generate().unwrap(),
        rcgen::KeyPair::generate().unwrap(),
        rcgen::KeyPair::generate().unwrap(),
    );
    std::fs::write(&cfg.cert, cert(&k1)).unwrap();
    std::fs::write(cfg.next_key.as_ref().unwrap(), k2.public_key_pem()).unwrap();
    let cell = DaneCell::default();
    refresh_files(&cfg, &cell);
    let s = cell.get().unwrap();
    assert_eq!(s.name, "_25._tcp.mx.example");
    let (r1, r2) = (s.current.clone().unwrap(), s.next.clone().unwrap());
    assert_ne!(r1, r2);

    published(&cell, &[&r1, &r2]);
    std::fs::write(&cfg.cert, cert(&k2)).unwrap();
    refresh_files(&cfg, &cell);
    let s = cell.get().unwrap();
    assert_eq!(s.current.as_ref(), Some(&r2));
    assert_eq!(s.unmatched_swaps, 0, "the next key was published");

    std::fs::write(&cfg.cert, cert(&k3)).unwrap();
    refresh_files(&cfg, &cell);
    assert_eq!(cell.get().unwrap().unmatched_swaps, 1);

    // unchanged files count nothing more
    refresh_files(&cfg, &cell);
    assert_eq!(cell.get().unwrap().unmatched_swaps, 1);
}

#[test]
fn a_swap_with_nothing_published_is_not_counted() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), false);
    std::fs::write(&cfg.cert, cert(&rcgen::KeyPair::generate().unwrap())).unwrap();
    let cell = DaneCell::default();
    refresh_files(&cfg, &cell);
    std::fs::write(&cfg.cert, cert(&rcgen::KeyPair::generate().unwrap())).unwrap();
    refresh_files(&cfg, &cell);
    let s = cell.get().unwrap();
    assert_eq!(s.unmatched_swaps, 0);
    assert_eq!(s.next, None);
}

fn reply(id: u16, ad: bool, records: &[[u8; 3]]) -> Vec<u8> {
    let mut m = Message::new(id, MessageType::Response, OpCode::Query);
    m.metadata.authentic_data = ad;
    let name = Name::from_ascii("_25._tcp.mx.example.").unwrap();
    for r in records {
        let data = TLSA::new(
            CertUsage::from(r[0]),
            Selector::from(r[1]),
            Matching::from(r[2]),
            vec![0xab, 0x01],
        );
        m.add_answer(Record::from_rdata(name.clone(), 3600, RData::TLSA(data)));
    }
    m.to_vec().unwrap()
}

#[test]
fn an_answer_reads_as_its_ad_bit_and_sorted_records() {
    let wire = reply(7, true, &[[3, 1, 1], [2, 0, 1]]);
    assert_eq!(
        parse_answer(&wire, 7).unwrap(),
        (
            true,
            vec!["2 0 1 ab01".to_string(), "3 1 1 ab01".to_string()]
        )
    );
    assert_eq!(
        parse_answer(&reply(7, false, &[]), 7).unwrap(),
        (false, vec![])
    );
    assert!(
        parse_answer(&wire, 8).is_err(),
        "a reply to another id is not ours"
    );
}
