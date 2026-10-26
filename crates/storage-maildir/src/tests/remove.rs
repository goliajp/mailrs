//! Unlinking a message by the name it was delivered under.

use crate::{Maildir, remove};

#[test]
fn remove_finds_a_file_whatever_flags_it_gained() {
    let tmp = super::tmpdir();
    let inbox = Maildir::create(tmp.path()).unwrap();
    let id = inbox.deliver(b"Subject: x\r\n\r\nbody").unwrap();
    // read since delivery: moved to cur/ and renamed with its flags
    std::fs::rename(
        tmp.path().join("new").join(&id.0),
        tmp.path().join("cur").join(format!("{}:2,S", id.0)),
    )
    .unwrap();
    let copy = Maildir::create(tmp.path().join(".Sent"))
        .unwrap()
        .deliver(b"Subject: x\r\n\r\nbody")
        .unwrap();

    assert!(remove(tmp.path(), &id.0).unwrap(), "the read inbox copy");
    assert!(remove(tmp.path(), &format!(".Sent/{}", copy.0)).unwrap());
    assert!(
        !remove(tmp.path(), &id.0).unwrap(),
        "already gone is not an error"
    );
    assert!(!remove(tmp.path(), "").unwrap());
    assert!(
        std::fs::read_dir(tmp.path().join("cur"))
            .unwrap()
            .next()
            .is_none()
    );
}
