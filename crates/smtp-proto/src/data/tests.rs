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

#[test]
fn empty_data() {
    assert_eq!(unstuff_data(b""), b"");
}

#[test]
fn only_terminator() {
    // just the terminator line — produces empty output
    assert_eq!(unstuff_data(b".\r\n"), b"");
}

#[test]
fn no_terminator_in_data() {
    // data without the ".\r\n" terminator — no fallback path (\n not found)
    // the final chunk without \n is still processed by the None arm
    let input = b"partial line without newline";
    let result = unstuff_data(input);
    assert_eq!(result, b"partial line without newline");
}

#[test]
fn data_with_multiple_dot_stuffed_lines() {
    let input = b"..first\r\n..second\r\n.\r\n";
    let expected = b".first\r\n.second\r\n";
    assert_eq!(unstuff_data(input), expected);
}

#[test]
fn unstuff_line_empty_slice() {
    // empty slice — not the terminator, not double-dot, just returned as-is
    assert_eq!(unstuff_line(b""), Some(&b""[..]));
}

#[test]
fn unstuff_line_single_dot_no_crlf() {
    // "." without \r\n is not the terminator
    assert_eq!(unstuff_line(b"."), Some(&b"."[..]));
}

#[test]
fn unstuff_line_content_starting_with_dot_not_stuffed() {
    // ".foo" — single dot prefix, not double-dot, returned as-is
    assert_eq!(unstuff_line(b".foo\r\n"), Some(&b".foo\r\n"[..]));
}
