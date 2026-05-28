//! ckpt 4 sieve-core — differential test against Stalwart's
//! `sieve-rs` (the oracle this engine will eventually replace).
//!
//! Builds the same action sequence out of both engines (mapped to
//! a shared `NormalizedAction` enum that drops engine-specific
//! metadata) and asserts equality. Slice 3 grows the corpus from
//! 30 → 60+ to ramp differential parity toward the ckpt 4 → 5
//! trigger gate of 200/200.

mod common;

use common::{ours, sieve_rs};

// --- Message fixtures (module-level consts to keep corpus fns small) ---

const MSG_SPAM: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: spam offer\r\n\
\r\n\
hello\r\n";

const MSG_CLEAN: &[u8] = b"\
From: Bob <bob@trusted.com>\r\n\
To: alice@example.com\r\n\
Subject: meeting tomorrow\r\n\
\r\n\
agenda attached\r\n";

// Long header to exercise :contains over 150+ char header values.
const MSG_LONGHDR: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: very long subject line that goes on for many characters including the NEEDLE_TOKEN word and continues with more filler text after that point to ensure it exceeds one hundred and fifty chars\r\n\
\r\n\
body\r\n";

// Folded Subject (RFC 5322 §2.2.3). The unfolder must collapse
// `\r\n ` / `\r\n\t` so `:contains` sees a single logical line.
const MSG_FOLDED: &[u8] = b"\
From: Alice <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: starts here\r\n continues on next line\r\n\twith tab continuation too\r\n\
\r\n\
body\r\n";

// From with quoted display-name containing a comma — exercises
// address-extraction in `address :localpart`.
const MSG_QUOTEDNAME: &[u8] = b"\
From: \"Alice, Sr.\" <alice@example.com>\r\n\
To: bob@dest.com\r\n\
Subject: hello\r\n\
\r\n\
body\r\n";

type CorpusRow = (&'static str, &'static str, &'static [u8]);

/// Slice 1/2 corpus — the original 32 rows seeded when the engine
/// was first built. Kept stable; new coverage goes in `corpus_slice3`.
fn corpus_slice12() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_clean = MSG_CLEAN;
    vec![
        ("explicit_keep", "keep;", msg_spam),
        ("explicit_discard", "discard;", msg_spam),
        (
            "fileinto",
            r#"require ["fileinto"]; fileinto "Junk";"#,
            msg_spam,
        ),
        (
            "header_is_spam",
            r#"if header :is "Subject" "spam offer" { discard; }"#,
            msg_spam,
        ),
        (
            "header_is_no_match",
            r#"if header :is "Subject" "nothing-here" { discard; }"#,
            msg_spam,
        ),
        (
            "header_contains_spam",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "header_contains_clean",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" { fileinto "Spam"; }
               else { keep; }"#,
            msg_clean,
        ),
        (
            "size_over_small",
            "if size :over 1 { discard; }",
            msg_spam,
        ),
        (
            "size_under_huge",
            "if size :under 100K { discard; }",
            msg_spam,
        ),
        (
            "exists_subject",
            r#"if exists "Subject" { discard; }"#,
            msg_spam,
        ),
        // --- slice 2 additions ---
        (
            "header_matches_glob_star_prefix",
            r#"if header :matches "Subject" "*offer*" { discard; }"#,
            msg_spam,
        ),
        (
            "header_matches_glob_question_mark",
            r#"if header :matches "Subject" "spam ?ffer" { discard; }"#,
            msg_spam,
        ),
        (
            "not_invert_spam",
            r#"if not header :is "Subject" "nothing" { discard; }"#,
            msg_spam,
        ),
        (
            "not_invert_clean",
            r#"if not header :is "Subject" "meeting tomorrow" { discard; }"#,
            msg_clean,
        ),
        (
            "allof_two_true",
            r#"if allof(header :contains "Subject" "spam", exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "allof_one_false",
            r#"if allof(header :contains "Subject" "spam", header :is "From" "wrong") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_one_true",
            r#"if anyof(header :is "Subject" "no-match", header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_all_false",
            r#"if anyof(header :is "Subject" "no-match-1", header :is "Subject" "no-match-2") { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_match",
            r#"if address :localpart "From" "alice" { discard; }"#,
            msg_spam,
        ),
        (
            "address_localpart_no_match",
            r#"if address :localpart "From" "carol" { discard; }"#,
            msg_spam,
        ),
        (
            "address_domain_match",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_to_list_localpart",
            r#"require ["fileinto"];
               if address :localpart "To" "bob" { fileinto "ToBob"; } else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_first_match",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match" { fileinto "A"; }
               elsif header :contains "Subject" "spam" { fileinto "Spam"; }
               elsif header :contains "Subject" "offer" { fileinto "Ads"; }
               else { keep; }"#,
            msg_spam,
        ),
        (
            "elsif_chain_else_branch",
            r#"require ["fileinto"];
               if header :is "Subject" "no-match" { fileinto "A"; }
               elsif header :is "Subject" "another-no" { fileinto "B"; }
               else { keep; }"#,
            msg_clean,
        ),
        (
            "stop_short_circuit",
            r#"if header :contains "Subject" "spam" { discard; stop; }
               keep;"#,
            msg_spam,
        ),
        (
            "redirect_then_keep_unguarded",
            r#"redirect "forward@example.com";"#,
            msg_spam,
        ),
        (
            "reject_with_reason",
            r#"require ["reject"]; reject "policy violation";"#,
            msg_spam,
        ),
        (
            "case_insensitive_is",
            r#"if header :is "Subject" "SPAM OFFER" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_missing",
            r#"if exists "X-Spam-Score" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_present",
            r#"if exists ["From", "To", "Subject"] { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_partial",
            r#"if exists ["From", "To", "X-Missing"] { discard; }"#,
            msg_spam,
        ),
        (
            "nested_if_inside_if",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :is "From" "Alice <alice@example.com>" { fileinto "SpamFromAlice"; }
                 else { fileinto "Spam"; }
               }"#,
            msg_spam,
        ),
    ]
}

/// Slice 3 corpus — 33 new scripts pushing differential parity
/// toward the ckpt 4 → 5 trigger gate of 200/200. Categorised
/// inline with comments so it's easy to add more per category
/// in future slices.
fn corpus_slice3() -> Vec<CorpusRow> {
    let msg_spam = MSG_SPAM;
    let msg_longhdr = MSG_LONGHDR;
    let msg_folded = MSG_FOLDED;
    let msg_quotedname = MSG_QUOTEDNAME;
    let _ = MSG_CLEAN; // suppress unused-const warning if this fn doesn't reference it
    vec![
        // :matches glob edge cases
        (
            "matches_star_middle",
            r#"if header :matches "Subject" "spam*offer" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_multi_stars",
            r#"if header :matches "From" "*alice*example*" { discard; }"#,
            msg_spam,
        ),
        (
            "matches_question_no_extra_char",
            r#"if header :matches "Subject" "spamoffer?" { discard; }"#,
            msg_spam,
        ),
        // address :all + multi-header
        (
            "address_all_list_to_match",
            r#"if address :all ["From", "To"] "bob@dest.com" { discard; }"#,
            msg_spam,
        ),
        (
            "address_all_list_no_match",
            r#"if address :all ["From", "To"] "carol@other.com" { discard; }"#,
            msg_spam,
        ),
        // size precise boundaries — msg_spam is 81 bytes
        (
            "size_over_below_boundary",
            "if size :over 80 { discard; }",
            msg_spam,
        ),
        (
            "size_over_at_boundary",
            "if size :over 81 { discard; }",
            msg_spam,
        ),
        // nested if 3 deep
        (
            "nested_if_3_deep_match",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :contains "From" "alice" {
                   if exists "To" { fileinto "Triple"; }
                 }
               }"#,
            msg_spam,
        ),
        (
            "nested_if_3_deep_inner_miss",
            r#"require ["fileinto"];
               if header :contains "Subject" "spam" {
                 if header :contains "From" "alice" {
                   if exists "X-Missing" { fileinto "Triple"; }
                 }
               }"#,
            msg_spam,
        ),
        // header :is with string-list on both sides (cross product)
        (
            "header_is_list_left_match",
            r#"if header :is ["From", "To"] "bob@dest.com" { discard; }"#,
            msg_spam,
        ),
        (
            "header_is_list_both_sides",
            r#"if header :is ["From", "Subject"] ["meeting tomorrow", "spam offer"] { discard; }"#,
            msg_spam,
        ),
        // multi-action sequences
        (
            "multi_fileinto_sequence",
            r#"require ["fileinto"]; fileinto "A"; fileinto "B";"#,
            msg_spam,
        ),
        (
            "fileinto_then_explicit_keep",
            r#"require ["fileinto"]; fileinto "Backup"; keep;"#,
            msg_spam,
        ),
        // implicit keep when test fires but no action emitted
        (
            "if_test_true_empty_block",
            r#"if exists "Subject" { }"#,
            msg_spam,
        ),
        (
            "if_outer_true_inner_false_no_action",
            r#"if exists "Subject" {
                 if exists "X-Missing" { discard; }
               }"#,
            msg_spam,
        ),
        // long header value :contains (150+ chars)
        (
            "long_header_contains_needle",
            r#"if header :contains "Subject" "NEEDLE_TOKEN" { discard; }"#,
            msg_longhdr,
        ),
        (
            "long_header_contains_absent",
            r#"if header :contains "Subject" "ABSENT_PHRASE" { discard; }"#,
            msg_longhdr,
        ),
        // empty string comparisons
        (
            "is_subject_against_empty_string",
            r#"if header :is "Subject" "" { discard; }"#,
            msg_spam,
        ),
        (
            "is_missing_header_against_empty_string",
            r#"if header :is "X-Empty" "" { discard; }"#,
            msg_spam,
        ),
        // allof with nested not
        (
            "allof_not_and_exists_true",
            r#"if allof(not header :is "Subject" "newsletter", exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "allof_not_false_short_circuit",
            r#"if allof(not header :is "Subject" "spam offer", exists "From") { discard; }"#,
            msg_spam,
        ),
        // anyof 3 branches with only third true
        (
            "anyof_three_only_third_true",
            r#"if anyof(header :is "Subject" "x", header :is "Subject" "y", header :contains "Subject" "spam") { discard; }"#,
            msg_spam,
        ),
        (
            "anyof_three_all_false",
            r#"if anyof(header :is "Subject" "x", header :is "Subject" "y", header :is "Subject" "z") { discard; }"#,
            msg_spam,
        ),
        // address :localpart with quoted display name containing comma
        (
            "address_localpart_quoted_display_name",
            r#"if address :localpart "From" "alice" { discard; }"#,
            msg_quotedname,
        ),
        (
            "address_domain_quoted_display_name",
            r#"if address :domain "From" "example.com" { discard; }"#,
            msg_quotedname,
        ),
        // size :over 0 — true for any non-empty body
        (
            "size_over_zero",
            "if size :over 0 { discard; }",
            msg_spam,
        ),
        // folded header unfolds before :contains
        (
            "folded_header_contains_continuation",
            r#"if header :contains "Subject" "continues on next line" { discard; }"#,
            msg_folded,
        ),
        (
            "folded_header_contains_tab_continuation",
            r#"if header :contains "Subject" "with tab continuation" { discard; }"#,
            msg_folded,
        ),
        // misc safety nets
        (
            "case_insensitive_contains_upper",
            r#"if header :contains "Subject" "OFFER" { discard; }"#,
            msg_spam,
        ),
        (
            "exists_multi_all_missing",
            r#"if exists ["X-Foo", "X-Bar"] { discard; }"#,
            msg_spam,
        ),
        (
            "allof_anyof_combined",
            r#"if allof(anyof(header :is "Subject" "x", header :contains "Subject" "spam"), exists "From") { discard; }"#,
            msg_spam,
        ),
        (
            "if_no_match_then_top_level_keep",
            r#"if header :is "Subject" "no-match" { discard; } keep;"#,
            msg_spam,
        ),
    ]
}

/// Combined corpus driven by the diff test.
fn corpus() -> Vec<CorpusRow> {
    let mut all = corpus_slice12();
    all.extend(corpus_slice3());
    all
}

#[test]
fn engines_agree_on_corpus() {
    let mut disagreements = Vec::new();
    for (label, script, msg) in corpus() {
        let a = ours(script, msg);
        let b = sieve_rs(script, msg);
        if a != b {
            disagreements.push((label, a, b));
        }
    }
    assert!(
        disagreements.is_empty(),
        "engine disagreement: {disagreements:#?}",
    );
}
