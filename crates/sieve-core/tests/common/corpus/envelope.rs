//! RFC 5228 §5.4 envelope test — envelope-aware corpus rows.
//! Distinct from the standard corpus because each row carries
//! caller-supplied envelope state (MAIL FROM / RCPT TO / Auth)
//! that both engines must be given for the comparison to be fair.

use super::super::EnvelopeRow;

const MSG: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: hello\r\n\
\r\n\
body\r\n";

pub fn corpus() -> Vec<EnvelopeRow> {
    vec![
        // --- envelope from match ---
        (
            "envelope_from_localpart_match",
            r#"require ["envelope"];
               if envelope :localpart "from" "alice" { discard; }"#,
            MSG,
            &[("from", "alice@example.com")],
        ),
        (
            "envelope_from_domain_match",
            r#"require ["envelope"];
               if envelope :domain "from" "example.com" { discard; }"#,
            MSG,
            &[("from", "alice@example.com")],
        ),
        (
            "envelope_from_full_match",
            r#"require ["envelope"];
               if envelope :all "from" "alice@example.com" { discard; }"#,
            MSG,
            &[("from", "alice@example.com")],
        ),
        (
            "envelope_from_no_match",
            r#"require ["envelope"];
               if envelope :is "from" "carol@other.com" { discard; }"#,
            MSG,
            &[("from", "alice@example.com")],
        ),
        // --- envelope to match (single recipient) ---
        (
            "envelope_to_localpart_match",
            r#"require ["envelope"];
               if envelope :localpart "to" "bob" { discard; }"#,
            MSG,
            &[("to", "bob@dest.com")],
        ),
        // --- envelope to with multiple recipients ---
        (
            "envelope_to_multi_one_match",
            r#"require ["envelope"];
               if envelope :localpart "to" "carol" { discard; }"#,
            MSG,
            &[
                ("to", "bob@dest.com"),
                ("to", "carol@other.com"),
                ("to", "dave@third.com"),
            ],
        ),
        (
            "envelope_to_multi_no_match",
            r#"require ["envelope"];
               if envelope :localpart "to" "frank" { discard; }"#,
            MSG,
            &[("to", "bob@dest.com"), ("to", "carol@other.com")],
        ),
        // --- envelope test with no envelope provided (empty state) ---
        (
            "envelope_from_empty_returns_false",
            r#"require ["envelope"];
               if envelope :is "from" "anyone@example.com" { discard; }"#,
            MSG,
            &[],
        ),
        // --- envelope :matches glob ---
        (
            "envelope_from_matches_glob",
            r#"require ["envelope"];
               if envelope :matches :all "from" "*@example.com" { discard; }"#,
            MSG,
            &[("from", "alice@example.com")],
        ),
        // --- envelope :contains ---
        (
            "envelope_to_contains_partial",
            r#"require ["envelope"];
               if envelope :contains :all "to" "@dest" { discard; }"#,
            MSG,
            &[("to", "bob@dest.com")],
        ),
        // --- envelope string-list of part names ---
        (
            "envelope_from_or_to_either_match",
            r#"require ["envelope"];
               if envelope :localpart ["from", "to"] "bob" { discard; }"#,
            MSG,
            &[("from", "alice@example.com"), ("to", "bob@dest.com")],
        ),
        // --- combined with body test ---
        (
            "envelope_then_subject",
            r#"require ["envelope", "fileinto"];
               if allof(envelope :domain "from" "example.com",
                        header :contains "Subject" "hello") { fileinto "FromUs"; }"#,
            MSG,
            &[("from", "alice@example.com")],
        ),
    ]
}
