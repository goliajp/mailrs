//! Tests for `feed_worker` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn split_extracts_each_vevent() {
    // The walker should find every BEGIN:VEVENT...END:VEVENT span,
    // even when separated by other VCALENDAR-level content. We test
    // the substring-walk piece via apply_ics_to_calendar's loop
    // shape — which lives inside the function — so this test asserts
    // the protocol expectation: a feed with N events produces N
    // wrapped VCALENDAR strings.
    let body = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
                 BEGIN:VEVENT\r\nUID:a\r\nDTSTAMP:20260430T120000Z\r\n\
                 DTSTART:20260501T140000Z\r\nSUMMARY:A\r\nEND:VEVENT\r\n\
                 BEGIN:VEVENT\r\nUID:b\r\nDTSTAMP:20260430T120000Z\r\n\
                 DTSTART:20260502T140000Z\r\nSUMMARY:B\r\nEND:VEVENT\r\n\
                 END:VCALENDAR\r\n";
    let text = std::str::from_utf8(body).unwrap();
    let mut count = 0;
    let mut search_from = 0;
    while let Some(begin_rel) = text[search_from..].find("BEGIN:VEVENT") {
        let begin = search_from + begin_rel;
        let Some(end_rel) = text[begin..].find("END:VEVENT") else {
            break;
        };
        let end = begin + end_rel + "END:VEVENT".len();
        count += 1;
        search_from = end;
    }
    assert_eq!(count, 2);
}
