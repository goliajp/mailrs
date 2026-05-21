use crate::types::MessageMeta;
use sqlx::Row;

/// build a user_address filter clause and collect bind values
/// returns (sql_fragment, bind_values) where bind values start at `start_idx`
pub(crate) fn build_user_filter(user: &str, domains: Option<&[String]>, start_idx: u32) -> (String, Vec<String>) {
    if let Some(doms) = domains
        && !doms.is_empty() {
            let placeholders: Vec<String> = doms.iter().enumerate()
                .map(|(i, _)| format!("${}", start_idx + i as u32))
                .collect();
            let sql = format!(
                "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                placeholders.join(",")
            );
            return (sql, doms.to_vec());
        }
    (format!("mb.user_address = ${start_idx}"), vec![user.to_string()])
}

/// convert a tuple row to MessageMeta
// the 15-tuple matches the column order in the messages-table SELECT;
// extracting a named struct just for this helper would duplicate the
// MessageMeta type for no clarity gain.
#[allow(clippy::type_complexity)]
pub(crate) fn row_to_message_meta(
    r: (i64, i64, i32, String, String, String, String, i64, i32, i32, i64, String, String, String, i64),
) -> MessageMeta {
    MessageMeta {
        id: r.0,
        mailbox_id: r.1,
        uid: r.2 as u32,
        maildir_id: r.3,
        sender: r.4,
        recipients: r.5,
        subject: r.6,
        date: r.7,
        size: r.8 as u32,
        flags: r.9 as u32,
        internal_date: r.10,
        message_id: r.11,
        in_reply_to: r.12,
        thread_id: r.13,
        modseq: r.14 as u64,
        user_address: String::new(),
        importance_level: String::from("normal"),
        importance_score: 0.0,
        is_bulk_sender: false,
        has_tracking_pixel: false,
        new_content: None,
    }
}

/// convert a PgRow to MessageMeta (for queries with >16 columns)
pub(crate) fn row_to_message_meta_from_row(r: sqlx::postgres::PgRow) -> MessageMeta {
    MessageMeta {
        id: r.get::<i64, _>(0),
        mailbox_id: r.get::<i64, _>(1),
        uid: r.get::<i32, _>(2) as u32,
        maildir_id: r.get::<String, _>(3),
        sender: r.get::<String, _>(4),
        recipients: r.get::<String, _>(5),
        subject: r.get::<String, _>(6),
        date: r.get::<i64, _>(7),
        size: r.get::<i32, _>(8) as u32,
        flags: r.get::<i32, _>(9) as u32,
        internal_date: r.get::<i64, _>(10),
        message_id: r.get::<String, _>(11),
        in_reply_to: r.get::<String, _>(12),
        thread_id: r.get::<String, _>(13),
        modseq: r.get::<i64, _>(14) as u64,
        user_address: r.get::<String, _>(15),
        importance_level: r.get::<String, _>(16),
        importance_score: r.get::<f32, _>(17),
        is_bulk_sender: r.get::<bool, _>(18),
        has_tracking_pixel: r.get::<bool, _>(19),
        new_content: r.get::<Option<String>, _>(20),
    }
}

/// extract a raw header value from RFC 5322 message bytes
pub(crate) fn extract_header_value(data: &[u8], name: &str) -> String {
    let text = String::from_utf8_lossy(data);
    let prefix = format!("{name}:");
    for line in text.lines() {
        if line.len() > prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            return line[prefix.len()..].trim().to_string();
        }
        if line.is_empty() {
            break;
        }
    }
    String::new()
}

/// read raw message bytes from maildir
pub(crate) fn read_raw_from_maildir(maildir_root: &str, user: &str, maildir_id: &str) -> Option<Vec<u8>> {
    let (local, domain) = user.split_once('@')?;
    let path = format!("{maildir_root}/{domain}/{local}");
    let md = mailrs_maildir::Maildir::open(&path);

    let find_in = |entries: Vec<mailrs_maildir::Entry>| -> Option<Vec<u8>> {
        entries
            .into_iter()
            .find(|e| e.id.to_string() == maildir_id)
            .and_then(|e| std::fs::read(&e.path).ok())
    };

    find_in(md.scan_cur().unwrap_or_default())
        .or_else(|| find_in(md.scan_new().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConversationSummary, FlagAction, Mailbox, MessageMeta, FLAG_SEEN};

    #[test]
    fn extract_header_value_basic() {
        let msg = b"From: alice@example.com\r\nSubject: Hello World\r\n\r\nbody";
        assert_eq!(extract_header_value(msg, "Subject"), "Hello World");
        assert_eq!(extract_header_value(msg, "From"), "alice@example.com");
    }

    #[test]
    fn extract_header_value_case_insensitive() {
        let msg = b"subject: hello world\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "hello world");
    }

    #[test]
    fn extract_header_value_missing() {
        let msg = b"From: alice@example.com\r\n\r\nbody";
        assert_eq!(extract_header_value(msg, "Subject"), "");
    }

    #[test]
    fn extract_header_value_stops_at_empty_line() {
        let msg = b"From: alice@example.com\r\n\r\nSubject: in body";
        assert_eq!(extract_header_value(msg, "Subject"), "");
    }

    #[test]
    fn extract_header_value_trims_whitespace() {
        let msg = b"Subject:   lots of spaces   \r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "lots of spaces");
    }

    #[test]
    fn extract_header_value_empty_message() {
        assert_eq!(extract_header_value(b"", "Subject"), "");
    }

    // ---- build_user_filter tests ----

    #[test]
    fn build_user_filter_no_domains() {
        let (sql, binds) = build_user_filter("alice@example.com", None, 1);
        assert_eq!(sql, "mb.user_address = $1");
        assert_eq!(binds, vec!["alice@example.com"]);
    }

    #[test]
    fn build_user_filter_empty_domains() {
        let (sql, binds) = build_user_filter("alice@example.com", Some(&[]), 1);
        assert_eq!(sql, "mb.user_address = $1");
        assert_eq!(binds, vec!["alice@example.com"]);
    }

    #[test]
    fn build_user_filter_single_domain() {
        let domains = vec!["example.com".to_string()];
        let (sql, binds) = build_user_filter("alice@example.com", Some(&domains), 1);
        assert_eq!(
            sql,
            "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ($1))"
        );
        assert_eq!(binds, vec!["example.com"]);
    }

    #[test]
    fn build_user_filter_multiple_domains() {
        let domains = vec!["a.com".to_string(), "b.com".to_string(), "c.com".to_string()];
        let (sql, binds) = build_user_filter("user@a.com", Some(&domains), 1);
        assert_eq!(
            sql,
            "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ($1,$2,$3))"
        );
        assert_eq!(binds, vec!["a.com", "b.com", "c.com"]);
    }

    #[test]
    fn build_user_filter_custom_start_idx() {
        let (sql, binds) = build_user_filter("alice@example.com", None, 5);
        assert_eq!(sql, "mb.user_address = $5");
        assert_eq!(binds, vec!["alice@example.com"]);

        let domains = vec!["x.com".to_string(), "y.com".to_string()];
        let (sql2, binds2) = build_user_filter("u@x.com", Some(&domains), 3);
        assert_eq!(
            sql2,
            "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ($3,$4))"
        );
        assert_eq!(binds2, vec!["x.com", "y.com"]);
    }

    // ---- row_to_message_meta tests ----

    #[test]
    fn row_to_message_meta_converts_correctly() {
        let row = (
            42i64, 7i64, 100i32,
            "maildir-abc".to_string(),
            "sender@test.com".to_string(),
            "rcpt@test.com".to_string(),
            "Test Subject".to_string(),
            1700000000i64, 2048i32, 1i32, 1700000001i64,
            "<msg-001@test.com>".to_string(),
            "<parent@test.com>".to_string(),
            "thread-xyz".to_string(),
            5i64,
        );
        let meta = row_to_message_meta(row);
        assert_eq!(meta.id, 42);
        assert_eq!(meta.mailbox_id, 7);
        assert_eq!(meta.uid, 100);
        assert_eq!(meta.maildir_id, "maildir-abc");
        assert_eq!(meta.sender, "sender@test.com");
        assert_eq!(meta.recipients, "rcpt@test.com");
        assert_eq!(meta.subject, "Test Subject");
        assert_eq!(meta.date, 1700000000);
        assert_eq!(meta.size, 2048);
        assert_eq!(meta.flags, 1);
        assert_eq!(meta.internal_date, 1700000001);
        assert_eq!(meta.message_id, "<msg-001@test.com>");
        assert_eq!(meta.in_reply_to, "<parent@test.com>");
        assert_eq!(meta.thread_id, "thread-xyz");
        assert_eq!(meta.modseq, 5);
        assert_eq!(meta.user_address, ""); // default empty
    }

    #[test]
    fn row_to_message_meta_defaults() {
        // row_to_message_meta sets default importance fields
        let row = (
            1i64, 2i64, 3i32,
            "mid".to_string(), "s".to_string(), "r".to_string(), "sub".to_string(),
            0i64, 0i32, 0i32, 0i64,
            "".to_string(), "".to_string(), "".to_string(), 0i64,
        );
        let meta = row_to_message_meta(row);
        assert_eq!(meta.user_address, "");
        assert_eq!(meta.importance_level, "normal");
        assert_eq!(meta.importance_score, 0.0);
        assert!(!meta.is_bulk_sender);
        assert!(!meta.has_tracking_pixel);
        assert_eq!(meta.new_content, None);
    }

    // ---- MessageMeta clone/debug tests ----

    #[test]
    fn message_meta_clone() {
        let meta = MessageMeta {
            id: 1, mailbox_id: 2, uid: 3, maildir_id: "abc".into(),
            sender: "s@t.com".into(), recipients: "r@t.com".into(),
            subject: "sub".into(), date: 100, size: 50, flags: FLAG_SEEN,
            internal_date: 101, message_id: "mid".into(),
            in_reply_to: "irt".into(), thread_id: "tid".into(),
            modseq: 42, user_address: "u@t.com".into(),
            importance_level: "normal".into(), importance_score: 0.0,
            is_bulk_sender: false, has_tracking_pixel: false, new_content: None,
        };
        let cloned = meta.clone();
        assert_eq!(cloned.id, meta.id);
        assert_eq!(cloned.subject, meta.subject);
        assert_eq!(cloned.flags, meta.flags);
        assert_eq!(cloned.user_address, meta.user_address);
    }

    #[test]
    fn message_meta_debug() {
        let meta = MessageMeta {
            id: 1, mailbox_id: 2, uid: 3, maildir_id: "abc".into(),
            sender: "s".into(), recipients: "r".into(), subject: "sub".into(),
            date: 0, size: 0, flags: 0, internal_date: 0,
            message_id: "".into(), in_reply_to: "".into(), thread_id: "".into(),
            modseq: 0, user_address: "".into(),
            importance_level: "normal".into(), importance_score: 0.0,
            is_bulk_sender: false, has_tracking_pixel: false, new_content: None,
        };
        let debug = format!("{:?}", meta);
        assert!(debug.contains("MessageMeta"));
        assert!(debug.contains("abc"));
    }

    // ---- ConversationSummary tests ----

    #[test]
    fn conversation_summary_clone() {
        let cs = ConversationSummary {
            thread_id: "t1".into(), subject: "Hello".into(),
            participants: "alice,bob".into(), message_count: 5,
            unread_count: 2, last_date: 1700000000, category: "general".into(),
            flagged: true, snippet: "preview text".into(),
            pinned: false, archived: false,
            importance_level: "normal".into(), importance_score: 0.0, requires_action: false,
            last_sender: "alice".into(),
            sent_count: 0,
        };
        let cloned = cs.clone();
        assert_eq!(cloned.thread_id, "t1");
        assert_eq!(cloned.message_count, 5);
        assert_eq!(cloned.unread_count, 2);
        assert!(cloned.flagged);
        assert!(!cloned.pinned);
        assert!(!cloned.archived);
        assert_eq!(cloned.snippet, "preview text");
    }

    #[test]
    fn conversation_summary_debug() {
        let cs = ConversationSummary {
            thread_id: "t1".into(), subject: "Hi".into(),
            participants: "a".into(), message_count: 1,
            unread_count: 0, last_date: 0, category: "promo".into(),
            flagged: false, snippet: "".into(),
            pinned: true, archived: true,
            importance_level: "normal".into(), importance_score: 0.0, requires_action: false,
            last_sender: "a".into(),
            sent_count: 0,
        };
        let debug = format!("{:?}", cs);
        assert!(debug.contains("ConversationSummary"));
        assert!(debug.contains("promo"));
    }

    // ---- Mailbox tests ----

    #[test]
    fn mailbox_clone_and_debug() {
        let mb = Mailbox {
            id: 10, user: "bob@test.com".into(), name: "INBOX".into(),
            uidvalidity: 12345, uidnext: 99, highest_modseq: 50,
        };
        let cloned = mb.clone();
        assert_eq!(cloned.id, 10);
        assert_eq!(cloned.user, "bob@test.com");
        assert_eq!(cloned.name, "INBOX");
        assert_eq!(cloned.uidvalidity, 12345);
        assert_eq!(cloned.uidnext, 99);
        assert_eq!(cloned.highest_modseq, 50);

        let debug = format!("{:?}", mb);
        assert!(debug.contains("Mailbox"));
        assert!(debug.contains("INBOX"));
    }

    // ---- extract_header_value edge cases ----

    #[test]
    fn extract_header_value_no_crlf() {
        let msg = b"Subject: Unix style\n\nbody here";
        assert_eq!(extract_header_value(msg, "Subject"), "Unix style");
    }

    #[test]
    fn extract_header_value_multiple_colons() {
        let msg = b"Subject: Re: Re: Important: urgent\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "Re: Re: Important: urgent");
    }

    #[test]
    fn extract_header_value_first_match_wins() {
        let msg = b"Subject: First\r\nSubject: Second\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "First");
    }

    #[test]
    fn extract_header_value_only_header_name_no_value() {
        // "Subject:" with nothing after => trim to empty
        let msg = b"Subject:\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "");
    }

    #[test]
    fn extract_header_value_utf8_content() {
        let msg = "Subject: 你好世界\r\n\r\nbody".as_bytes();
        assert_eq!(extract_header_value(msg, "Subject"), "你好世界");
    }

    #[test]
    fn extract_header_value_similar_prefix_no_match() {
        // "Subject-Alt" should not match "Subject"
        let msg = b"Subject-Alt: nope\r\nSubject: yes\r\n\r\n";
        assert_eq!(extract_header_value(msg, "Subject"), "yes");
    }

    // ---- FlagAction tests ----

    #[test]
    fn flag_action_clone_copy_eq() {
        let a = FlagAction::Set;
        let b = a;
        assert_eq!(a, b);
        assert_eq!(FlagAction::Add, FlagAction::Add);
        assert_ne!(FlagAction::Set, FlagAction::Remove);
    }

    #[test]
    fn flag_action_debug() {
        assert_eq!(format!("{:?}", FlagAction::Set), "Set");
        assert_eq!(format!("{:?}", FlagAction::Add), "Add");
        assert_eq!(format!("{:?}", FlagAction::Remove), "Remove");
    }

    // ---- EmailAnalysisRow tests ----

    #[test]
    fn email_analysis_row_clone_and_debug() {
        let row = crate::types::EmailAnalysisRow {
            message_id: 42,
            category: "finance".into(),
            risk_score: 75,
            risk_reason: "suspicious sender".into(),
            summary: "wire transfer request".into(),
            people: serde_json::json!(["Alice", "Bob"]),
            dates: serde_json::json!(["2026-03-01"]),
            amounts: serde_json::json!(["$1000"]),
            action_items: serde_json::json!(["review"]),
            model_version: "v2".into(),
            clean_text: "some cleaned text".into(),
            requires_action: true,
            sender_intent: "request".into(),
            action_deadline: Some("2026-03-15".into()),
        };
        let cloned = row.clone();
        assert_eq!(cloned.message_id, 42);
        assert_eq!(cloned.category, "finance");
        assert_eq!(cloned.risk_score, 75);
        assert_eq!(cloned.risk_reason, "suspicious sender");

        let debug = format!("{:?}", row);
        assert!(debug.contains("EmailAnalysisRow"));
        assert!(debug.contains("finance"));
    }
}
