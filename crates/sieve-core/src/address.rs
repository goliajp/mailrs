//! RFC 5228 §5.1 address-part helpers — extracted from `eval.rs`
//! so the evaluator stays under the file-size limit.

/// Which slice of an `addr-spec` an `address` test consults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddressPart {
    /// Whole `local@domain` form (the RFC 5228 `:all` default).
    All,
    /// Local part only (left of `@`).
    LocalPart,
    /// Domain part only (right of `@`).
    Domain,
    /// RFC 5233 `:user` — the local-part minus the detail sub-part
    /// and the `+` joining delimiter. E.g. for
    /// `alice+work@example.com`, returns `"alice"`. When the
    /// local-part has no `+`, returns the entire local-part.
    User,
    /// RFC 5233 `:detail` — the detail sub-part of the local-part
    /// (the slice after the first `+`). E.g. for
    /// `alice+work@example.com`, returns `"work"`. When the
    /// local-part has no `+`, returns the empty string (RFC 5233
    /// §5.2: matches `:is ""` and nothing else).
    Detail,
}

/// Pick the address-part tag from a `tags` slice, defaulting to
/// `:all` per RFC 5228 §5.1.
pub(crate) fn address_part_from_tags(tags: &[String]) -> AddressPart {
    for t in tags {
        match t.as_str() {
            "all" => return AddressPart::All,
            "localpart" => return AddressPart::LocalPart,
            "domain" => return AddressPart::Domain,
            "user" => return AddressPart::User,
            "detail" => return AddressPart::Detail,
            _ => {}
        }
    }
    AddressPart::All
}

/// Project an `addr-spec` onto the requested address part.
///
/// Returns `None` only for `Detail` when the local-part has no `+`
/// (RFC 5233 §5.2 — the detail sub-part is undefined). All other
/// variants always return `Some`; an empty result (e.g. the domain
/// of a malformed address) is `Some("")`. The caller is expected to
/// skip candidates that return `None` so the test fails for them
/// (RFC 5233 §5.2's `:is ""`-on-undefined exception is intentionally
/// **not** honored — it disagrees with sieve-rs, which is the
/// oracle the wrapper swap will route to).
pub(crate) fn scope_to_part(addr: &str, part: AddressPart) -> Option<String> {
    match part {
        AddressPart::All => Some(addr.to_string()),
        AddressPart::LocalPart => Some(local_part(addr).to_string()),
        AddressPart::Domain => Some(
            addr.split_once('@')
                .map(|(_, d)| d.to_string())
                .unwrap_or_default(),
        ),
        AddressPart::User => Some(match local_part(addr).split_once('+') {
            // RFC 5233 §5.1 — :user is local-part split on the
            // **first** `+`. Multi-`+` local-parts keep "alice" as
            // the user and the remainder belongs to the detail.
            Some((user, _)) => user.to_string(),
            None => local_part(addr).to_string(),
        }),
        // sieve-rs (via mail-parser) splits `:detail` on the **last**
        // `+` — `alice+work+sub` → user = "alice", detail = "sub".
        // The asymmetry vs :user is intentional; we mirror it so the
        // wrapper swap doesn't introduce behavioural drift.
        AddressPart::Detail => local_part(addr)
            .rsplit_once('+')
            .map(|(_, detail)| detail.to_string()),
    }
}

fn local_part(addr: &str) -> &str {
    addr.split_once('@').map(|(l, _)| l).unwrap_or(addr)
}

/// Naive address extractor: pulls the bare addr-spec(s) out of a
/// raw RFC 5322 address header value. Supports the two common
/// shapes (`alice@example.com` and `Name <alice@example.com>`)
/// plus comma-separated lists. Quoted display names are kept
/// trimmed.
pub(crate) fn extract_addresses(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for piece in value.split(',') {
        let trim = piece.trim();
        if let Some(open) = trim.rfind('<')
            && trim.ends_with('>')
        {
            out.push(trim[open + 1..trim.len() - 1].trim().to_string());
        } else if trim.contains('@') {
            out.push(trim.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_default_all() {
        assert_eq!(address_part_from_tags(&[]), AddressPart::All);
    }

    #[test]
    fn part_localpart() {
        assert_eq!(
            address_part_from_tags(&["localpart".into()]),
            AddressPart::LocalPart,
        );
    }

    #[test]
    fn part_user_distinct_from_localpart() {
        // RFC 5233 §5.1 — :user no longer aliases to :localpart;
        // it produces the local-part minus +detail.
        assert_eq!(
            address_part_from_tags(&["user".into()]),
            AddressPart::User,
        );
    }

    #[test]
    fn part_detail() {
        assert_eq!(
            address_part_from_tags(&["detail".into()]),
            AddressPart::Detail,
        );
    }

    #[test]
    fn scope_local_and_domain() {
        assert_eq!(
            scope_to_part("a@b.c", AddressPart::LocalPart),
            Some("a".into()),
        );
        assert_eq!(
            scope_to_part("a@b.c", AddressPart::Domain),
            Some("b.c".into()),
        );
        assert_eq!(
            scope_to_part("a@b.c", AddressPart::All),
            Some("a@b.c".into()),
        );
    }

    #[test]
    fn scope_user_with_plus_strips_detail() {
        // RFC 5233 §5.1 — :user is local-part minus +detail.
        assert_eq!(
            scope_to_part("alice+work@example.com", AddressPart::User),
            Some("alice".into()),
        );
    }

    #[test]
    fn scope_user_without_plus_returns_full_localpart() {
        // RFC 5233 §5.2 — when there is no detail, :user equals the
        // entire local-part.
        assert_eq!(
            scope_to_part("alice@example.com", AddressPart::User),
            Some("alice".into()),
        );
    }

    #[test]
    fn scope_detail_with_plus_returns_after_plus() {
        assert_eq!(
            scope_to_part("alice+work@example.com", AddressPart::Detail),
            Some("work".into()),
        );
    }

    #[test]
    fn scope_detail_without_plus_is_undefined() {
        // RFC 5233 §5.2 — undefined when no `+`. We model that as
        // `None` so the caller can skip the candidate; this matches
        // sieve-rs behaviour, which is the swap-time oracle.
        // (Strict RFC 5233 §5.2 would have `:is ""` succeed against
        // undefined detail. sieve-rs does not honor that exception
        // and neither do we — see scope_to_part doc.)
        assert_eq!(
            scope_to_part("alice@example.com", AddressPart::Detail),
            None,
        );
    }

    #[test]
    fn scope_detail_multi_plus_takes_last_segment() {
        // sieve-rs (mail-parser) splits :detail on the **last** `+`.
        // So `alice+work+sub@example.com` → user = "alice",
        // detail = "sub" (NOT "work+sub").
        assert_eq!(
            scope_to_part("alice+work+sub@example.com", AddressPart::Detail),
            Some("sub".into()),
        );
        assert_eq!(
            scope_to_part("alice+work+sub@example.com", AddressPart::User),
            Some("alice".into()),
        );
    }

    #[test]
    fn extract_bare_addr() {
        assert_eq!(extract_addresses("alice@example.com"), vec!["alice@example.com"]);
    }

    #[test]
    fn extract_angle_addr() {
        assert_eq!(
            extract_addresses("Alice <alice@example.com>"),
            vec!["alice@example.com"]
        );
    }

    #[test]
    fn extract_list_of_addrs() {
        assert_eq!(
            extract_addresses("bob@x.com, carol@y.com"),
            vec!["bob@x.com", "carol@y.com"]
        );
    }

    #[test]
    fn extract_quoted_display_name_with_comma() {
        // The comma inside quotes splits the piece, but the second
        // piece still contains the `<addr>` form and is recovered.
        assert_eq!(
            extract_addresses("\"Alice, Sr.\" <alice@example.com>"),
            vec!["alice@example.com"]
        );
    }
}
