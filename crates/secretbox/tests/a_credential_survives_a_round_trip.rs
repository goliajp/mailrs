//! What a store needs from this, stated as tests.

use mailrs_secretbox::{Error, Key, open, seal};

fn key() -> Key {
    Key::from_passphrase("a deployment key, any deployment key")
}

#[test]
fn what_goes_in_comes_out() {
    let k = key();
    let secret = b"1//0gLm-not-a-real-refresh-token";
    let sealed = seal(&k, secret).expect("seal");
    assert_eq!(open(&k, &sealed).expect("open"), secret);
}

#[test]
fn the_sealed_form_does_not_contain_the_secret() {
    let sealed = seal(&key(), b"hunter2-and-then-some").expect("seal");
    assert!(!sealed.contains("hunter2"), "{sealed}");
}

/// Sealing twice must not produce the same bytes, or a store leaks
/// which two accounts share a password.
#[test]
fn the_same_secret_seals_differently_every_time() {
    let k = key();
    let a = seal(&k, b"same").expect("seal");
    let b = seal(&k, b"same").expect("seal");
    assert_ne!(a, b);
    assert_eq!(open(&k, &a).unwrap(), open(&k, &b).unwrap());
}

/// The failure that matters: a value sealed under a key that is no
/// longer configured must say so, not decrypt to noise and not be
/// mistaken for a corrupt row.
#[test]
fn a_different_key_is_refused_by_name() {
    let sealed = seal(&key(), b"secret").expect("seal");
    let other = Key::from_passphrase("some other deployment");
    assert!(matches!(open(&other, &sealed), Err(Error::WrongKey { .. })));
}

#[test]
fn a_tampered_ciphertext_is_refused() {
    let k = key();
    let sealed = seal(&k, b"secret").expect("seal");
    // Flip the last base64 character to something else.
    let mut bad = sealed.clone();
    let last = bad.pop().unwrap();
    bad.push(if last == 'A' { 'B' } else { 'A' });
    assert!(open(&k, &bad).is_err());
}

#[test]
fn nonsense_is_refused_rather_than_panicking() {
    let k = key();
    for junk in [
        "",
        "v1",
        "v1.aa.bb",
        "not base64 at all",
        "v9.aaaaaaaaaaaaaaaa.zz",
    ] {
        assert!(open(&k, junk).is_err(), "{junk} decoded");
    }
}

/// Two keys are told apart in a store without either being revealed.
#[test]
fn the_fingerprint_identifies_the_key_but_is_not_the_key() {
    let a = Key::from_passphrase("one");
    let b = Key::from_passphrase("two");
    assert_ne!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.fingerprint().len(), 16, "eight bytes, hex");
    assert!(seal(&a, b"x").unwrap().contains(a.fingerprint()));
}

/// An empty secret is a secret. Storing one must not be special.
#[test]
fn an_empty_secret_round_trips() {
    let k = key();
    let sealed = seal(&k, b"").expect("seal");
    assert_eq!(open(&k, &sealed).expect("open"), b"");
}
