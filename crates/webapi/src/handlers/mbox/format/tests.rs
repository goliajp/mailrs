use super::*;

const PLAIN: &[u8] = b"From: a@x\nSubject: hi\n\nFrom the start\n>From quoted\nbody\n";

fn split_all(mbox: &[u8], chunk: usize) -> Vec<Parsed> {
    let mut s = Splitter::default();
    let mut out = Vec::new();
    for c in mbox.chunks(chunk) {
        out.extend(s.push(c));
    }
    out.extend(s.finish());
    out
}

#[test]
fn a_message_comes_back_as_it_went_out_whatever_the_chunk_size() {
    let crlf = b"From: b@x\r\nSubject: two\r\n\r\nline\r\nFrom here\r\n".to_vec();
    let mut mbox = Vec::new();
    write_message(&mut mbox, PLAIN, 1_700_000_000, Folder::Archive, true);
    write_message(&mut mbox, &crlf, 1_700_000_100, Folder::Sent, false);
    for chunk in [1, 2, 7, 64, 1 << 20] {
        let got = split_all(&mbox, chunk);
        assert_eq!(got.len(), 2, "chunk {chunk}");
        assert_eq!(got[0].raw, PLAIN, "chunk {chunk}");
        assert_eq!(got[0].folder, Folder::Archive);
        assert!(got[0].seen);
        assert_eq!(got[1].raw, crlf, "chunk {chunk}");
        assert_eq!(got[1].folder, Folder::Sent);
        assert!(!got[1].seen);
    }
}

#[test]
fn from_lines_in_a_body_are_quoted_so_they_do_not_split_it() {
    let mut mbox = Vec::new();
    write_message(&mut mbox, PLAIN, 0, Folder::Inbox, false);
    let text = String::from_utf8(mbox).unwrap();
    assert!(text.contains("\n>From the start\n"), "{text}");
    assert!(text.contains("\n>>From quoted\n"), "{text}");
    assert_eq!(text.matches("\nFrom ").count(), 0, "{text}");
}

#[test]
fn gmail_labels_decide_the_folder_and_the_read_state() {
    let msg = |labels: &str| {
        format!("From 1@xxx Mon Jan 1 00:00:00 2024\nX-Gmail-Labels: {labels}\nSubject: s\n\nb\n")
    };
    let cases = [
        ("Inbox,Unread", Folder::Inbox, false),
        ("Inbox,Important", Folder::Inbox, true),
        ("Sent", Folder::Sent, true),
        ("\"Category Updates\",Opened", Folder::Archive, true),
        ("Spam,Unread", Folder::Discarded, false),
        ("Trash", Folder::Discarded, true),
    ];
    for (labels, folder, seen) in cases {
        let got = split_all(msg(labels).as_bytes(), 1 << 20);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].folder, got[0].seen), (folder, seen), "{labels}");
        // the labels stay in the message; only the export's own headers go
        assert!(String::from_utf8_lossy(&got[0].raw).contains("X-Gmail-Labels"));
    }
}

#[test]
fn a_plain_mbox_reads_status_and_files_into_the_inbox() {
    let mbox = b"From x Mon Jan 1 00:00:00 2024\nSubject: a\nStatus: RO\n\none\n\nFrom y Mon Jan 1 00:00:00 2024\nSubject: b\n\ntwo\n";
    let got = split_all(mbox, 5);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].raw, b"Subject: a\n\none\n");
    assert!(got[0].seen);
    assert_eq!(got[1].raw, b"Subject: b\n\ntwo\n");
    assert!(!got[1].seen);
    assert_eq!(got[1].folder, Folder::Inbox);
}

#[test]
fn text_before_the_first_separator_is_not_a_message() {
    assert!(split_all(b"garbage\nmore\n", 3).is_empty());
}
