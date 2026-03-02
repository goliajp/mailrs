use crate::data::{unstuff_data, unstuff_line};

// --- unstuff_line ---

#[test]
fn normal_line() {
    assert_eq!(unstuff_line(b"Hello\r\n"), Some(&b"Hello\r\n"[..]));
}

#[test]
fn double_dot() {
    assert_eq!(unstuff_line(b"..foo\r\n"), Some(&b".foo\r\n"[..]));
}

#[test]
fn single_dot_terminator() {
    assert_eq!(unstuff_line(b".\r\n"), None);
}

#[test]
fn triple_dot() {
    assert_eq!(unstuff_line(b"...bar\r\n"), Some(&b"..bar\r\n"[..]));
}

#[test]
fn dot_only_line() {
    assert_eq!(unstuff_line(b"..\r\n"), Some(&b".\r\n"[..]));
}

#[test]
fn no_dot() {
    assert_eq!(
        unstuff_line(b"normal text\r\n"),
        Some(&b"normal text\r\n"[..])
    );
}

// --- unstuff_data (complete message) ---

#[test]
fn complete_message() {
    let input = b"Subject: test\r\n\r\n..Hello\r\n..\r\nnormal\r\n.\r\n";
    let expected = b"Subject: test\r\n\r\n.Hello\r\n.\r\nnormal\r\n";
    assert_eq!(unstuff_data(input), expected);
}
