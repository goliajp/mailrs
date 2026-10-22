//! Who a conversation is with, as a list row shows it.

/// The participants of a thread from its senders in arrival order: each
/// comma-separated part once, at its first appearance, compared without
/// case and trimmed. Both stores build the row's list with this, so the
/// same mail lists the same people in the same order.
pub fn union<'a>(senders: impl IntoIterator<Item = &'a str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in senders.into_iter().flat_map(|s| s.split(',')) {
        let p = part.trim();
        if !p.is_empty() && !out.iter().any(|s| s.eq_ignore_ascii_case(p)) {
            out.push(p);
        }
    }
    out.join(",")
}

#[cfg(test)]
mod tests {
    use super::union;

    #[test]
    fn first_appearance_wins_and_case_does_not_count() {
        assert_eq!(
            union(["b@x.jp", "a@x.jp,B@X.JP", " c@x.jp ", ""]),
            "b@x.jp,a@x.jp,c@x.jp"
        );
        assert_eq!(union([]), "");
    }
}
