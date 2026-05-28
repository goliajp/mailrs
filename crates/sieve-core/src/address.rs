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
}

/// Pick the address-part tag from a `tags` slice, defaulting to
/// `:all` per RFC 5228 §5.1.
pub(crate) fn address_part_from_tags(tags: &[String]) -> AddressPart {
    for t in tags {
        match t.as_str() {
            "all" => return AddressPart::All,
            "localpart" | "user" => return AddressPart::LocalPart,
            "domain" => return AddressPart::Domain,
            _ => {}
        }
    }
    AddressPart::All
}

/// Project an `addr-spec` onto the requested address part.
pub(crate) fn scope_to_part(addr: &str, part: AddressPart) -> String {
    match part {
        AddressPart::All => addr.to_string(),
        AddressPart::LocalPart => addr
            .split_once('@')
            .map(|(l, _)| l.to_string())
            .unwrap_or_else(|| addr.to_string()),
        AddressPart::Domain => addr
            .split_once('@')
            .map(|(_, d)| d.to_string())
            .unwrap_or_default(),
    }
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
    fn part_localpart_or_user() {
        assert_eq!(
            address_part_from_tags(&["localpart".into()]),
            AddressPart::LocalPart,
        );
        assert_eq!(
            address_part_from_tags(&["user".into()]),
            AddressPart::LocalPart,
        );
    }

    #[test]
    fn scope_local_and_domain() {
        assert_eq!(scope_to_part("a@b.c", AddressPart::LocalPart), "a");
        assert_eq!(scope_to_part("a@b.c", AddressPart::Domain), "b.c");
        assert_eq!(scope_to_part("a@b.c", AddressPart::All), "a@b.c");
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
