//! A message that says it is a reply and belongs to no thread.
//!
//! A mail client that answers or forwards a message writes the subject
//! marker and the thread headers together: `Re:` and `In-Reply-To:` /
//! `References:` come from the same button. A subject that starts with
//! the marker while the message names no message it answers was written
//! by hand to look like a conversation already under way.
//!
//! Only the markers clients write: `Re`, `Fw`, `Fwd`, `AW` (German
//! Outlook), `TR` (French Outlook), in any case, followed by an ASCII or
//! a full-width colon (`Re：`, as Japanese and Chinese input methods
//! type it). A word that merely starts with those letters (`Rent:`,
//! `Trial:`) is not a marker.

/// The reply and forward markers, lowercase.
const MARKERS: &[&str] = &["re", "fw", "fwd", "aw", "tr"];

/// Whether `subject` starts with a reply or forward marker.
#[must_use]
pub fn has_reply_marker(subject: &str) -> bool {
    let s = subject.trim_start();
    MARKERS.iter().any(|m| {
        s.get(..m.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(m))
            && s[m.len()..]
                .trim_start_matches(' ')
                .starts_with([':', '：'])
    })
}

/// Whether the message claims to be a reply but names nothing it answers:
/// the subject carries a marker, and neither `In-Reply-To` nor
/// `References` has a value.
#[must_use]
pub fn without_thread(subject: &str, in_reply_to: Option<&str>, references: Option<&str>) -> bool {
    let empty = |v: Option<&str>| v.is_none_or(|v| v.trim().is_empty());
    has_reply_marker(subject) && empty(in_reply_to) && empty(references)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_markers_clients_write_are_read() {
        for s in [
            "Re: invoice",
            "RE: PARTNERSHIP PROPOSITION",
            "re:no space",
            "Fw: minutes",
            "FWD: slides",
            "AW: Angebot",
            "TR: facture",
            "Re：お見積りの件",
            "  Re : spaced",
            "Re: Re: twice",
        ] {
            assert!(has_reply_marker(s), "{s}");
        }
    }

    #[test]
    fn a_word_that_starts_like_a_marker_is_not_one() {
        for s in [
            "Rent: due",
            "Trial: ends",
            "Award: yours",
            "Fwiw: nothing",
            "Report",
            "",
            "回复：会议",
            "Ｒｅ：全角",
        ] {
            assert!(!has_reply_marker(s), "{s}");
        }
    }

    #[test]
    fn a_reply_that_names_its_thread_is_not_one() {
        let id = Some("<a@b>");
        assert!(without_thread("Re: invoice", None, None));
        assert!(without_thread("Re: invoice", Some("  "), Some("")));
        assert!(!without_thread("Re: invoice", id, None));
        assert!(!without_thread("Re: invoice", None, id));
        assert!(!without_thread("invoice", None, None));
    }
}
