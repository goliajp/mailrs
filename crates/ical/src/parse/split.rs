//! Splitting a feed into one document per event.

/// Split a VCALENDAR document into one self-contained document per VEVENT.
///
/// A subscribed feed is a single VCALENDAR holding every event, and
/// [`crate::parse_invite`] reads one. Re-parsing the whole document per event
/// is quadratic on a year of meetings, so each `BEGIN:VEVENT…END:VEVENT`
/// span is lifted out and wrapped in a minimal VCALENDAR, with every
/// `VTIMEZONE` of the document: an event's `TZID` refers to them, and a
/// part without them reads a Windows zone name by the name alone.
///
/// The monolith's feed worker had this inline
/// (`server/src/calendar/feed_worker.rs`); it lives here because the second
/// worker needs the same reading of the same RFC, and two copies of a
/// document splitter is how the two lanes stop agreeing.
///
/// Malformed input yields fewer events rather than an error: an unterminated
/// `BEGIN:VEVENT` ends the walk, since nothing after it can be trusted to be
/// a complete event.
///
/// ```
/// let doc = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nEND:VEVENT\r\n\
///            BEGIN:VEVENT\r\nUID:b\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
/// let parts = mailrs_ical::parse::split_vevents(doc);
/// assert_eq!(parts.len(), 2);
/// assert!(parts[0].contains("UID:a"));
/// assert!(parts[0].starts_with("BEGIN:VCALENDAR"));
/// ```
pub fn split_vevents(text: &str) -> Vec<String> {
    const BEGIN: &str = "BEGIN:VEVENT";
    const END: &str = "END:VEVENT";
    let zones = blocks(text, "BEGIN:VTIMEZONE", "END:VTIMEZONE").join("\r\n");
    let zones = match zones.is_empty() {
        true => zones,
        false => zones + "\r\n",
    };
    blocks(text, BEGIN, END)
        .into_iter()
        .map(|event| {
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//mailrs//feed-import//EN\r\n{zones}{event}\r\nEND:VCALENDAR\r\n"
            )
        })
        .collect()
}

/// Every complete `begin`…`end` span of `text`, in order. An unterminated
/// one ends the walk.
fn blocks<'a>(text: &'a str, begin: &str, end: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(begin_rel) = text[from..].find(begin) {
        let start = from + begin_rel;
        let Some(end_rel) = text[start..].find(end) else {
            break;
        };
        let stop = start + end_rel + end.len();
        out.push(&text[start..stop]);
        from = stop;
    }
    out
}

#[cfg(test)]
mod split_tests {
    use super::split_vevents;

    #[test]
    fn each_event_is_a_complete_document() {
        let doc = "BEGIN:VCALENDAR\r\n\
                   BEGIN:VEVENT\r\nUID:one\r\nSUMMARY:First\r\nEND:VEVENT\r\n\
                   BEGIN:VEVENT\r\nUID:two\r\nSUMMARY:Second\r\nEND:VEVENT\r\n\
                   END:VCALENDAR\r\n";
        let parts = split_vevents(doc);
        assert_eq!(parts.len(), 2);
        for (part, uid) in parts.iter().zip(["one", "two"]) {
            assert!(part.starts_with("BEGIN:VCALENDAR"));
            assert!(part.trim_end().ends_with("END:VCALENDAR"));
            assert!(part.contains(&format!("UID:{uid}")));
        }
        // Each carries only its own event.
        assert!(!parts[0].contains("UID:two"));
    }

    #[test]
    fn a_document_with_no_events_yields_none() {
        assert!(split_vevents("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").is_empty());
        assert!(split_vevents("").is_empty());
    }

    /// An unterminated event ends the walk: what follows cannot be read as
    /// a complete event, and guessing produces a plausible wrong one.
    #[test]
    fn an_unterminated_event_is_dropped_with_what_follows() {
        let doc = "BEGIN:VEVENT\r\nUID:ok\r\nEND:VEVENT\r\n\
                   BEGIN:VEVENT\r\nUID:truncated\r\n";
        let parts = split_vevents(doc);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].contains("UID:ok"));
    }
    #[test]
    fn each_event_keeps_the_documents_time_zones() {
        let doc = "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time\r\n\
                   END:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:a\r\nEND:VEVENT\r\n\
                   BEGIN:VEVENT\r\nUID:b\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        for part in split_vevents(doc) {
            assert!(part.contains("BEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time"));
        }
    }
}
