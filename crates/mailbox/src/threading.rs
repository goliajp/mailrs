/// strip angle brackets from a Message-ID
pub fn normalize_message_id(id: &str) -> &str {
    let trimmed = id.trim();
    trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed)
}

/// determine the thread_id for a message
///
/// - if `in_reply_to` is empty, start a new thread using `own_id`
/// - if `in_reply_to` has a value, look up the parent's thread_id
///   - if found, reuse the parent's thread_id
///   - if not found, use `in_reply_to` as thread_id (orphan reply)
pub fn resolve_thread_id<F>(own_id: &str, in_reply_to: &str, lookup: F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    if in_reply_to.is_empty() {
        return own_id.to_string();
    }
    match lookup(in_reply_to) {
        Some(tid) => tid,
        None => in_reply_to.to_string(),
    }
}

/// extract Message-ID header value from raw RFC 5322 bytes
pub fn extract_message_id(data: &[u8]) -> String {
    extract_header_raw(data, "message-id")
}

/// extract In-Reply-To header value from raw RFC 5322 bytes
pub fn extract_in_reply_to(data: &[u8]) -> String {
    extract_header_raw(data, "in-reply-to")
}

fn extract_header_raw(data: &[u8], name: &str) -> String {
    let text = String::from_utf8_lossy(data);
    let prefix_len = name.len() + 1; // "name:"
    for line in text.lines() {
        if line.len() > prefix_len && line.as_bytes()[name.len()] == b':'
            && line[..name.len()].eq_ignore_ascii_case(name) {
                let val = line[prefix_len..].trim();
                return normalize_message_id(val).to_string();
        }
        // empty line = end of headers
        if line.is_empty() {
            break;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_angle_brackets() {
        assert_eq!(normalize_message_id("<abc@host>"), "abc@host");
    }

    #[test]
    fn normalize_no_brackets() {
        assert_eq!(normalize_message_id("abc@host"), "abc@host");
    }

    #[test]
    fn normalize_whitespace() {
        assert_eq!(normalize_message_id("  <abc@host>  "), "abc@host");
    }

    #[test]
    fn resolve_new_thread() {
        let tid = resolve_thread_id("own@host", "", |_| None);
        assert_eq!(tid, "own@host");
    }

    #[test]
    fn resolve_existing_parent() {
        let tid = resolve_thread_id("own@host", "parent@host", |id| {
            assert_eq!(id, "parent@host");
            Some("root@host".to_string())
        });
        assert_eq!(tid, "root@host");
    }

    #[test]
    fn resolve_orphan_reply() {
        let tid = resolve_thread_id("own@host", "parent@host", |_| None);
        assert_eq!(tid, "parent@host");
    }

    #[test]
    fn extract_message_id_from_bytes() {
        let data = b"From: a@b.com\r\nMessage-ID: <123@host>\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_message_id(data), "123@host");
    }

    #[test]
    fn extract_in_reply_to_from_bytes() {
        let data = b"From: a@b.com\r\nIn-Reply-To: <parent@host>\r\n\r\nbody";
        assert_eq!(extract_in_reply_to(data), "parent@host");
    }

    #[test]
    fn extract_missing_header() {
        let data = b"From: a@b.com\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_message_id(data), "");
        assert_eq!(extract_in_reply_to(data), "");
    }

    #[test]
    fn extract_case_insensitive() {
        let data = b"message-id: <lower@host>\r\n\r\n";
        assert_eq!(extract_message_id(data), "lower@host");
    }

    #[test]
    fn normalize_only_open_bracket() {
        assert_eq!(normalize_message_id("<abc@host"), "<abc@host");
    }

    #[test]
    fn normalize_only_close_bracket() {
        assert_eq!(normalize_message_id("abc@host>"), "abc@host>");
    }

    #[test]
    fn normalize_empty_string() {
        assert_eq!(normalize_message_id(""), "");
    }

    #[test]
    fn normalize_empty_brackets() {
        assert_eq!(normalize_message_id("<>"), "");
    }

    #[test]
    fn normalize_nested_brackets() {
        assert_eq!(normalize_message_id("<<inner>>"), "<inner>");
    }

    #[test]
    fn resolve_uses_lookup_result() {
        let tid = resolve_thread_id("own@host", "parent@host", |id| {
            if id == "parent@host" {
                Some("thread-root".to_string())
            } else {
                None
            }
        });
        assert_eq!(tid, "thread-root");
    }

    #[test]
    fn resolve_empty_own_id_with_empty_reply_to() {
        let tid = resolve_thread_id("", "", |_| None);
        assert_eq!(tid, "");
    }

    #[test]
    fn extract_message_id_stops_at_empty_line() {
        let data = b"Subject: hi\r\n\r\nMessage-ID: <body@host>\r\n";
        assert_eq!(extract_message_id(data), "");
    }

    #[test]
    fn extract_message_id_upper_case_header() {
        let data = b"MESSAGE-ID: <UPPER@host>\r\n\r\n";
        assert_eq!(extract_message_id(data), "UPPER@host");
    }

    #[test]
    fn extract_in_reply_to_multiple_headers() {
        // should return first match
        let data = b"In-Reply-To: <first@host>\r\nIn-Reply-To: <second@host>\r\n\r\n";
        assert_eq!(extract_in_reply_to(data), "first@host");
    }

    #[test]
    fn extract_header_no_crlf() {
        let data = b"Message-ID: <no-crlf@host>";
        assert_eq!(extract_message_id(data), "no-crlf@host");
    }

    #[test]
    fn extract_header_lf_only() {
        let data = b"Message-ID: <lf@host>\n\nbody";
        assert_eq!(extract_message_id(data), "lf@host");
    }

    #[test]
    fn extract_empty_data() {
        assert_eq!(extract_message_id(b""), "");
    }

    // ===== Additional corner-case tests =====

    #[test]
    fn normalize_internal_brackets_preserved() {
        // brackets that aren't at the ends should be left in place
        assert_eq!(normalize_message_id("ab<cd>ef"), "ab<cd>ef");
    }

    #[test]
    fn normalize_only_whitespace() {
        // pure whitespace trims to empty
        assert_eq!(normalize_message_id("   "), "");
    }

    #[test]
    fn normalize_tab_and_newline_trim() {
        assert_eq!(normalize_message_id("\t<x@y>\n"), "x@y");
    }

    #[test]
    fn resolve_thread_id_lookup_never_called_for_empty_reply_to() {
        // Confirms the fast path: when in_reply_to is empty, lookup is not invoked.
        let called = std::cell::Cell::new(false);
        let tid = resolve_thread_id("own@host", "", |_| {
            called.set(true);
            Some("should-not-be-used".to_string())
        });
        assert_eq!(tid, "own@host");
        assert!(!called.get(), "lookup must not be invoked for empty in_reply_to");
    }

    #[test]
    fn resolve_thread_id_lookup_called_exactly_once_for_nonempty_reply_to() {
        let count = std::cell::Cell::new(0u32);
        let _ = resolve_thread_id("own@host", "parent@host", |_| {
            count.set(count.get() + 1);
            Some("root@host".to_string())
        });
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn resolve_thread_id_orphan_uses_normalized_reply_to_verbatim() {
        // The lookup miss path returns the in_reply_to *verbatim* — it is the
        // caller's responsibility to normalize before invoking.
        let tid = resolve_thread_id("own@host", "<not-normalized@host>", |_| None);
        assert_eq!(tid, "<not-normalized@host>", "orphan path does not auto-strip");
    }

    #[test]
    fn extract_header_with_leading_spaces_in_value() {
        // RFC 5322 allows whitespace between colon and value; ensure trim handles it
        let data = b"Message-ID:      <padded@host>\r\n\r\n";
        assert_eq!(extract_message_id(data), "padded@host");
    }

    #[test]
    fn extract_header_partial_match_does_not_collide() {
        // "Message-ID-Extra" should not match "Message-ID"
        let data = b"Message-ID-Extra: nope\r\nMessage-ID: <real@host>\r\n\r\n";
        assert_eq!(extract_message_id(data), "real@host");
    }

    #[test]
    fn extract_header_value_only_colon_no_value() {
        // header with colon but no value should normalize to empty
        let data = b"Message-ID:\r\n\r\nbody";
        assert_eq!(extract_message_id(data), "");
    }

    #[test]
    fn extract_in_reply_to_with_brackets_normalized() {
        // verify In-Reply-To extraction also strips brackets
        let data = b"In-Reply-To: <abc@def>\r\n\r\n";
        assert_eq!(extract_in_reply_to(data), "abc@def");
    }

    #[test]
    fn extract_header_invalid_utf8_falls_back_to_lossy() {
        // invalid UTF-8 in the body shouldn't panic
        let data: &[u8] = b"Message-ID: <ok@host>\r\nSubject: \xff\xfe garbled\r\n\r\nbody";
        assert_eq!(extract_message_id(data), "ok@host");
    }

    #[test]
    fn normalize_double_strip_idempotent() {
        // Running normalize twice yields the same result as once.
        let once = normalize_message_id("<abc@host>");
        let twice = normalize_message_id(once);
        assert_eq!(once, twice);
    }

    #[test]
    fn resolve_thread_id_chain_of_replies_uses_lookup_result() {
        // Simulates a small in-memory "table" the lookup consults — common usage shape.
        let table = std::collections::HashMap::from([
            ("a@host".to_string(), "thread-1".to_string()),
            ("b@host".to_string(), "thread-1".to_string()),
        ]);
        let t1 = resolve_thread_id("c@host", "a@host", |id| table.get(id).cloned());
        let t2 = resolve_thread_id("c@host", "b@host", |id| table.get(id).cloned());
        assert_eq!(t1, "thread-1");
        assert_eq!(t2, "thread-1", "two different parents in the same thread");
    }

    #[test]
    fn extract_header_only_newline_separator() {
        // Just one LF between header and body
        let data = b"Message-ID: <lf-only@host>\n";
        assert_eq!(extract_message_id(data), "lf-only@host");
    }

    #[test]
    fn extract_header_blank_data_before_header() {
        // RFC says first blank line ends headers — so a leading blank line means no headers
        let data = b"\r\nMessage-ID: <after-blank@host>\r\n";
        assert_eq!(extract_message_id(data), "");
    }
}
