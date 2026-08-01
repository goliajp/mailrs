//! Tests for `compose` — kept in their own file only because the module
//! they belong to is at the size limit. `file-size.md` counts a trailing
//! inline `#[cfg(test)] mod tests` as free, and these are two separate
//! named modules, so they do not qualify.

#![cfg(test)]

use crate::handlers::compose::*;

mod build_rfc5322_tests {
    use super::*;

    fn parts(body: &str, atts: Vec<Attachment>) -> ComposeParts {
        ComposeParts {
            from: "a@example.com".into(),
            to: vec!["b@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "hello".into(),
            body: body.into(),
            html_body: String::new(),
            in_reply_to: None,
            forward_message_id: None,
            forward_attachments_from: None,
            reply_to_thread_id: None,
            attachments: atts,
            scheduled_at: None,
        }
    }

    #[test]
    fn text_body_is_base64_not_8bit() {
        let (_mid, bytes) = build_rfc5322(&parts("hi 日本", vec![]), "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("Content-Transfer-Encoding: base64\r\n"));
        assert!(!s.contains("Content-Transfer-Encoding: 8bit"));
    }

    #[test]
    fn attachment_uses_rfc2231_for_japanese_filename() {
        let att = Attachment {
            filename: "取引明細.xlsx".into(),
            content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                .into(),
            bytes: b"hello".to_vec(),
        };
        let (_mid, bytes) = build_rfc5322(&parts("hi", vec![att]), "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(
            s.contains("filename*=UTF-8''"),
            "expected RFC 2231 filename*=; body =\n{s}"
        );
        assert!(s.contains("name*=UTF-8''"), "expected RFC 2231 name*=");
        assert!(
            !s.contains("filename=\"取引明細"),
            "raw UTF-8 must not appear"
        );
    }

    #[test]
    fn attachment_ascii_filename_stays_legacy_quoted() {
        let att = Attachment {
            filename: "report.pdf".into(),
            content_type: "application/pdf".into(),
            bytes: b"x".to_vec(),
        };
        let (_mid, bytes) = build_rfc5322(&parts("hi", vec![att]), "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("filename=\"report.pdf\""));
    }

    #[test]
    fn multipart_mixed_wraps_alternative_when_html_and_attachments() {
        let att = Attachment {
            filename: "a.txt".into(),
            content_type: "text/plain".into(),
            bytes: b"x".to_vec(),
        };
        let mut p = parts("plain", vec![att]);
        p.html_body = "<p>html</p>".into();
        let (_mid, bytes) = build_rfc5322(&p, "a@example.com");
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("multipart/mixed"));
        assert!(s.contains("multipart/alternative"));
    }
}

mod scheduled_at_tests {
    use crate::handlers::compose::parse_scheduled_at;

    /// An untouched form field is an empty string, and that means "send
    /// now" — not a client error.
    #[test]
    fn an_empty_field_is_not_scheduling_and_not_an_error() {
        assert_eq!(parse_scheduled_at(""), Ok(None));
        assert_eq!(parse_scheduled_at("   "), Ok(None));
    }

    #[test]
    fn epoch_seconds_parse() {
        assert_eq!(parse_scheduled_at("1785369273"), Ok(Some(1785369273)));
        assert_eq!(parse_scheduled_at(" 1785369273 "), Ok(Some(1785369273)));
    }

    /// The exact value the web composer sent. `.parse().ok()` turned this
    /// into `None`, and `None` means send now — so a mail scheduled for
    /// tomorrow went out at once, silently. It must be refused instead.
    #[test]
    fn the_iso_string_the_composer_used_to_send_is_refused_not_ignored() {
        assert_eq!(parse_scheduled_at("2026-07-30T10:00:00.000Z"), Err(()));
        assert_eq!(parse_scheduled_at("2026-07-30T10:00"), Err(()));
    }

    /// Any unreadable value is a rejection, for the same reason: dropping
    /// it means doing the one thing the caller was trying to avoid.
    #[test]
    fn a_value_that_cannot_be_read_is_refused() {
        assert_eq!(parse_scheduled_at("soon"), Err(()));
        assert_eq!(parse_scheduled_at("1785369273.5"), Err(()));
        assert_eq!(parse_scheduled_at("1e9"), Err(()));
    }

    /// The JSON path never had the silent-drop problem — serde refuses a
    /// string for an `i64` and axum answers 422 — but nothing recorded
    /// that, so a lenient custom deserializer added later would reintroduce
    /// the silence with every test still green.
    #[test]
    fn the_json_body_takes_an_integer_and_only_an_integer() {
        let ok: super::SendRequest =
            serde_json::from_str(r#"{"scheduled_at":1785369273}"#).expect("epoch seconds");
        assert_eq!(ok.scheduled_at, Some(1785369273));

        let absent: super::SendRequest = serde_json::from_str("{}").expect("absent is fine");
        assert_eq!(absent.scheduled_at, None);

        assert!(
            serde_json::from_str::<super::SendRequest>(
                r#"{"scheduled_at":"2026-07-30T10:00:00Z"}"#
            )
            .is_err(),
            "an ISO string must be refused, not coerced or dropped"
        );
    }

    /// A past epoch is legal at this layer: `enqueue_outbound_at` filters
    /// it with `> created` and sends immediately, which is the documented
    /// behaviour. Rejecting it here would be a second, disagreeing rule.
    #[test]
    fn a_past_epoch_parses_and_is_left_to_the_enqueue_step() {
        assert_eq!(parse_scheduled_at("1"), Ok(Some(1)));
        assert_eq!(parse_scheduled_at("-1"), Ok(Some(-1)));
    }
}
