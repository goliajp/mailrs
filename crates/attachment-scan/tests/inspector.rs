//! The inspector as a whole: hash facts, the allowlist, retractions.

use mailrs_attachment_scan::{Attachment, HashObservation, Inspector, Limits, Stance};

fn obs(id: &str, sha: &str, stance: Stance, retracts: Option<&str>) -> HashObservation {
    HashObservation {
        id: id.into(),
        at: 1,
        sha256: sha.into(),
        stance,
        retracts: retracts.map(String::from),
        reason: format!("{id} reason"),
        actor: None,
    }
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn att<'a>(name: &'a str, bytes: &'a [u8]) -> Attachment<'a> {
    Attachment {
        name,
        declared_type: "",
        bytes,
    }
}

#[test]
fn a_plain_pdf_has_no_findings() {
    let i = Inspector::build(&[], Limits::default(), 0);
    let f = &i.inspect(&[att("report.pdf", b"%PDF-1.7\n<</Type/Page>>")])[0];
    assert_eq!((f.part, f.extension.as_str()), (0, "pdf"));
    assert!(f.executable_name.is_none() && f.pdf_script.is_none() && f.hash_listed.is_none());
}

#[test]
fn a_listed_hash_is_named_and_an_allowed_file_is_left_out() {
    let bad: &[u8] = b"just some bytes";
    let good: &[u8] = b"MZ known good installer";
    let i = Inspector::build(
        &[
            obs("h1", &sha(bad), Stance::Deny, None),
            obs("h2", &sha(good), Stance::Allow, None),
            obs("h3", &sha(good), Stance::Deny, None),
        ],
        Limits::default(),
        0,
    );
    assert_eq!(i.denied_hashes(), 1);
    let facts = i.inspect(&[att("notes.txt", bad), att("setup.exe", good)]);
    assert_eq!(facts.len(), 1, "the allowed file is not there to judge");
    let listed = facts[0].hash_listed.as_ref().expect("h1 listed");
    assert_eq!(
        (listed.id.as_str(), listed.reason.as_str(), listed.age_days),
        ("h1", "h1 reason", 0)
    );
    assert_eq!(facts[0].sha256, sha(bad));
}

#[test]
fn a_retraction_takes_a_hash_back() {
    let bad: &[u8] = b"was thought bad";
    let i = Inspector::build(
        &[
            obs("h1", &sha(bad), Stance::Deny, None),
            obs("h2", "", Stance::Retract, Some("h1")),
        ],
        Limits::default(),
        0,
    );
    assert_eq!(i.denied_hashes(), 0);
    assert!(i.inspect(&[att("x.txt", bad)])[0].hash_listed.is_none());
}

#[test]
fn the_same_input_gives_the_same_facts() {
    let i = Inspector::build(&[], Limits::default(), 0);
    let parts = [
        att("a.exe", b"MZ"),
        att("b.pdf", b"%PDF-1.4 /JavaScript"),
        att("c.rar", b"Rar!\x1a\x07"),
    ];
    let first = i.inspect(&parts);
    assert_eq!(
        first[1].pdf_script.as_deref(),
        Some("/JavaScript in \"b.pdf\"")
    );
    assert_eq!(
        first[2].unlistable_archive.as_deref(),
        Some("a rar archive")
    );
    for _ in 0..3 {
        assert_eq!(i.inspect(&parts), first);
    }
}
