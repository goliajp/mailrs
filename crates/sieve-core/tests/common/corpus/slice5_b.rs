//! Slice 5 corpus part B — RFC 5233 `subaddress` extension.
//! Covers `:user` (local-part minus +detail) and `:detail` (the
//! detail sub-part) address-parts on the `address` test, plus the
//! §5.2 edge cases (no `+` → :user is full local-part, :detail is
//! the empty string with the `:is ""` exception).

use super::CorpusRow;

pub(super) fn corpus() -> Vec<CorpusRow> {
    let msg_with_plus: &[u8] = b"\
From: Alice <alice+work@example.com>\r\n\
To: bob+team@dest.com\r\n\
Subject: subaddress test\r\n\
\r\n\
body\r\n";
    let msg_no_plus: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: no detail\r\n\
\r\n\
body\r\n";
    let msg_multi_plus: &[u8] = b"\
From: Alice <alice+work+sub@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: multi plus\r\n\
\r\n\
body\r\n";
    vec![
        // --- :user with + present ---
        (
            "user_strips_plus_detail",
            r#"require ["subaddress"];
               if address :user "From" "alice" { discard; }"#,
            msg_with_plus,
        ),
        (
            "user_full_localpart_no_match",
            r#"require ["subaddress"];
               if address :user "From" "alice+work" { discard; }"#,
            msg_with_plus,
        ),
        // --- :user without + ---
        (
            "user_falls_through_to_localpart_when_no_plus",
            r#"require ["subaddress"];
               if address :user "From" "alice" { discard; }"#,
            msg_no_plus,
        ),
        // --- :detail with + present ---
        (
            "detail_extracts_after_plus",
            r#"require ["subaddress"];
               if address :detail "From" "work" { discard; }"#,
            msg_with_plus,
        ),
        (
            "detail_no_match_against_user",
            r#"require ["subaddress"];
               if address :detail "From" "alice" { discard; }"#,
            msg_with_plus,
        ),
        // --- :detail without + (sieve-rs treats as undefined → all
        //     match types miss, including `:is ""`. This deviates
        //     from a strict RFC 5233 §5.2 reading but matches the
        //     swap-time oracle. See slice 5.4 doc for the policy
        //     decision.) ---
        (
            "detail_is_anything_no_match_when_no_plus",
            r#"require ["subaddress"];
               if address :is :detail "From" "work" { discard; }"#,
            msg_no_plus,
        ),
        (
            "detail_contains_no_match_when_no_plus",
            r#"require ["subaddress"];
               if address :contains :detail "From" "anything" { discard; }"#,
            msg_no_plus,
        ),
        // --- :contains on :detail ---
        (
            "detail_contains_substring",
            r#"require ["subaddress"];
               if address :contains :detail "From" "or" { discard; }"#,
            msg_with_plus,
        ),
        (
            "detail_contains_no_match",
            r#"require ["subaddress"];
               if address :contains :detail "From" "zzz" { discard; }"#,
            msg_with_plus,
        ),
        // --- :matches on :detail ---
        (
            "detail_matches_glob",
            r#"require ["subaddress"];
               if address :matches :detail "From" "*ork*" { discard; }"#,
            msg_with_plus,
        ),
        // --- :matches on :user ---
        (
            "user_matches_glob",
            r#"require ["subaddress"];
               if address :matches :user "From" "ali?e" { discard; }"#,
            msg_with_plus,
        ),
        // --- Multi-plus: :user splits on first `+`, :detail on the
        //     **last** `+` (asymmetric per mail-parser; we mirror).
        (
            "multi_plus_user_first_segment",
            r#"require ["subaddress"];
               if address :user "From" "alice" { discard; }"#,
            msg_multi_plus,
        ),
        (
            "multi_plus_detail_takes_last_segment",
            r#"require ["subaddress"];
               if address :detail "From" "sub" { discard; }"#,
            msg_multi_plus,
        ),
        (
            "multi_plus_detail_first_segment_no_match",
            r#"require ["subaddress"];
               if address :detail "From" "work" { discard; }"#,
            msg_multi_plus,
        ),
        // --- :user on `To` header (multi-recipient) ---
        (
            "user_on_to_header_with_plus",
            r#"require ["subaddress", "fileinto"];
               if address :user "To" "bob" { fileinto "BobTeam"; }"#,
            msg_with_plus,
        ),
        // --- :detail combined with fileinto routing ---
        (
            "detail_route_into_folder",
            r#"require ["subaddress", "fileinto"];
               if address :detail "To" "team" { fileinto "Team-Inbox"; }"#,
            msg_with_plus,
        ),
    ]
}
