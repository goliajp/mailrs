//! Finding a message's file by the name it was delivered under.

use crate::{Maildir, find};

#[test]
fn find_names_a_file_whatever_flags_it_gained() {
    let tmp = super::tmpdir();
    let inbox = Maildir::create(tmp.path()).unwrap();
    let id = inbox.deliver(b"Subject: x\r\n\r\nbody").unwrap();
    // read since delivery: moved to cur/ and renamed with its flags
    let read = tmp.path().join("cur").join(format!("{}:2,S", id.0));
    std::fs::rename(tmp.path().join("new").join(&id.0), &read).unwrap();
    let copy = Maildir::create(tmp.path().join(".Sent"))
        .unwrap()
        .deliver(b"Subject: x\r\n\r\nbody")
        .unwrap();

    assert_eq!(find(tmp.path(), &id.0).unwrap(), Some(read.clone()));
    let sent = find(tmp.path(), &format!(".Sent/{}", copy.0)).unwrap();
    assert_eq!(
        sent,
        Some(tmp.path().join(".Sent").join("new").join(&copy.0))
    );
    std::fs::remove_file(&read).unwrap();
    assert_eq!(find(tmp.path(), &id.0).unwrap(), None, "gone");
    assert_eq!(find(tmp.path(), "").unwrap(), None);
}
