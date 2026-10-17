//! The reason string a Junk decision carries.

use super::PipelineInput;

/// Build the Junk-decision reason string with a single pre-sized
/// allocation instead of the `format!` macro's geometric String growth.
///
/// The Junk path was measured (criterion bench, M-series Mac, release)
/// at ~735 ns total vs. ~337 ns for the Accept path — the 2.4× gap is
/// entirely this string-build. Using `String::with_capacity` + `write!`
/// avoids both the intermediate `Vec<String>` from `matched_rules.join`
/// and the geometric resize cascade that `format!` does (16 → 32 → 64 …).
pub(super) fn build_junk_reason(
    input: &PipelineInput,
    total_score: f64,
    suspicious_score: f64,
) -> String {
    use std::fmt::Write as _;
    // Capacity for the prefix (~50 bytes) + numeric fields (~6 bytes
    // each w/ {:.1}) + a generous 64-byte budget for matched_rules.
    // Real-world reasons rarely exceed 150 bytes.
    let mut out = String::with_capacity(176);
    let _ = write!(
        out,
        "score {total_score:.1} >= {:.1} (content={:.1}, ptr={:.1}, ai={:.1}",
        input.spam_threshold, input.content_score, input.ptr_score, input.ai_score,
    );
    // Only mention the sender-trust contribution when it fired, so clean
    // mail's reason string stays as before.
    if suspicious_score > 0.0 {
        let _ = write!(out, ", sender=suspicious(+{suspicious_score:.1})");
    }
    // Named in the reason, because "why is this in Junk" has a very
    // different answer here than a content score does — and because a
    // false positive has to be findable in the log by the person who
    // needs to add a domain to the allow-list.
    // Named, because a false positive has to be findable in the log by
    // whoever needs to fix it.
    let fraud = input.fraud.score();
    if fraud > 0.0 {
        let _ = write!(out, ", {}(+{fraud:.1})", input.fraud.rules().join(", "));
    }
    out.push_str(", ");
    // Inline the rule-name join — avoid `matched_rules.join(", ")` which
    // builds an intermediate Vec<&str> + sums the lengths first.
    let mut first = true;
    for rule in &input.matched_rules {
        if !first {
            out.push_str(", ");
        }
        out.push_str(rule);
        first = false;
    }
    out.push(')');
    out
}
