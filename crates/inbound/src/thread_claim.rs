//! Whether a message says it is a reply and names no message it answers,
//! read the one way the receiver, the sweep and the offline checker share.

const HEAD_LIMIT: usize = 16 * 1024;

/// The decoded subject starts with a reply or forward marker, and neither
/// `In-Reply-To` nor `References` has a value —
/// [`mailrs_fraud::reply_claim::without_thread`].
pub fn reply_marker_without_thread(raw: &[u8]) -> bool {
    let (in_reply_to, references) = thread_headers(raw);
    mailrs_fraud::reply_claim::without_thread(
        &crate::identity::subject_header(raw),
        in_reply_to.as_deref(),
        references.as_deref(),
    )
}

/// `In-Reply-To` and `References`, unfolded: Exchange puts the value on
/// the continuation line and leaves the first one empty.
fn thread_headers(raw: &[u8]) -> (Option<String>, Option<String>) {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    // [In-Reply-To, References]
    let mut found: [Option<String>; 2] = [None, None];
    let mut current: Option<usize> = None;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(v) = current.and_then(|i| found[i].as_mut()) {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        let lower = line.to_ascii_lowercase();
        current = if lower.starts_with("in-reply-to:") {
            Some(0)
        } else if lower.starts_with("references:") {
            Some(1)
        } else {
            None
        };
        if let Some(i) = current {
            let value = line.split_once(':').map_or("", |(_, v)| v.trim());
            found[i] = Some(value.to_string());
        }
    }
    let [in_reply_to, references] = found;
    (in_reply_to, references)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_with_no_thread_headers_is_read() {
        let raw =
            b"From: a@b.example\r\nSubject: =?UTF-8?B?UmXvvJrjgYrnn6XjgonjgZs=?=\r\n\r\nbody\r\n";
        assert!(reply_marker_without_thread(raw));
    }

    #[test]
    fn a_folded_thread_header_counts() {
        let raw = b"Subject: RE: order\r\nIn-Reply-To:\r\n <x@y.example>\r\n\r\nbody\r\n";
        assert!(!reply_marker_without_thread(raw));
        let refs = b"Subject: Re: order\r\nReferences: <x@y.example>\r\n\r\n";
        assert!(!reply_marker_without_thread(refs));
    }

    #[test]
    fn an_empty_thread_header_names_nothing() {
        let raw = b"Subject: Re: order\r\nIn-Reply-To: \r\nReferences:\r\n\r\n";
        assert!(reply_marker_without_thread(raw));
    }

    #[test]
    fn a_header_in_the_body_is_not_a_header() {
        let raw = b"Subject: Re: order\r\n\r\nIn-Reply-To: <x@y.example>\r\n";
        assert!(reply_marker_without_thread(raw));
    }
}
