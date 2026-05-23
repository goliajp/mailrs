//! DKIM-Signature header parsing (RFC 6376 §3.5).

use crate::error::DkimError;

/// Algorithm announced in the `a=` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// `a=rsa-sha256` — RSA over SHA-256. ~99% of real-world DKIM.
    RsaSha256,
    /// `a=ed25519-sha256` — Ed25519 over SHA-256, per RFC 8463.
    /// Modern but rare; ~1% of real-world DKIM in 2026.
    Ed25519Sha256,
}

/// Canonicalization variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canon {
    /// `simple` — body: must end with one CRLF, ignore trailing
    /// empty lines; headers: untouched (whitespace preserved verbatim).
    Simple,
    /// `relaxed` — body: collapse internal WSP runs to one SP,
    /// strip trailing WSP, then apply simple; headers: lowercase
    /// name, unfold, collapse WSP, strip trailing WSP after value.
    Relaxed,
}

/// Parsed DKIM-Signature header. Borrows the `b=` (signature) and
/// `bh=` (body hash) base64 strings + the signed-header list etc.
/// Owned `String` is used for tag values that we may need to
/// case-fold or massage during verify (e.g. signed-header names get
/// lowercased for relaxed canon).
#[derive(Debug, Clone)]
pub struct DkimHeader {
    /// `v=` — version (must be "1" per RFC 6376).
    pub version: u32,
    /// `a=` — signature algorithm.
    pub algorithm: Algorithm,
    /// `b=` — base64-encoded signature bytes.
    pub signature_b64: String,
    /// `bh=` — base64-encoded body hash.
    pub body_hash_b64: String,
    /// `c=` — `(header_canon, body_canon)` tuple. Default
    /// `(Simple, Simple)` per spec.
    pub canon_header: Canon,
    /// see [`Self::canon_header`].
    pub canon_body: Canon,
    /// `d=` — signing domain (used in the selector DNS lookup).
    pub domain: String,
    /// `s=` — selector (used in `<s>._domainkey.<d>` TXT lookup).
    pub selector: String,
    /// `h=` — colon-separated list of signed header names, in the
    /// order they were signed. **Lowercased and trimmed** in
    /// parse so verifier doesn't have to.
    pub signed_headers: Vec<String>,
    /// `l=` — optional body length limit. Some signers sign only the
    /// first N bytes of the body to allow trailing additions.
    pub body_length: Option<u64>,
    /// `t=` — optional signature timestamp (seconds since epoch).
    pub timestamp: Option<u64>,
    /// `x=` — optional expiry (seconds since epoch). Verifier checks
    /// `now > x` → expired.
    pub expiration: Option<u64>,
    /// `i=` — optional identity (used for DMARC alignment but not
    /// for hash inputs).
    pub identity: Option<String>,
    /// `q=` — query method (default "dns/txt"). We only support
    /// "dns/txt"; anything else → UnsupportedAlgorithm.
    pub query_method: String,
}

impl DkimHeader {
    /// Parse a single `DKIM-Signature:` header value. Caller has already
    /// stripped the `DKIM-Signature:` prefix; this function expects the
    /// VALUE portion (everything after the first `:`).
    ///
    /// The header may contain folded continuation lines (CRLF + WSP);
    /// we unfold internally before parsing tags.
    pub fn parse(value: &str) -> Result<Self, DkimError> {
        let unfolded = unfold(value);
        let mut tags = parse_tag_list(&unfolded)?;

        // Required tags
        let v = tags
            .remove("v")
            .ok_or_else(|| DkimError::MissingTag("v".into()))?;
        let version: u32 = v
            .trim()
            .parse()
            .map_err(|_| DkimError::InvalidTag(format!("v={v}")))?;
        if version != 1 {
            return Err(DkimError::InvalidTag(format!("v={version}, expected 1")));
        }

        let a = tags
            .remove("a")
            .ok_or_else(|| DkimError::MissingTag("a".into()))?;
        let algorithm = match a.trim() {
            "rsa-sha256" => Algorithm::RsaSha256,
            "ed25519-sha256" => Algorithm::Ed25519Sha256,
            other => return Err(DkimError::UnsupportedAlgorithm(other.to_string())),
        };

        let b = tags
            .remove("b")
            .ok_or_else(|| DkimError::MissingTag("b".into()))?;
        // The b= value may contain WSP introduced by folding; strip all WSP
        let signature_b64 = strip_wsp(&b);

        let bh = tags
            .remove("bh")
            .ok_or_else(|| DkimError::MissingTag("bh".into()))?;
        let body_hash_b64 = strip_wsp(&bh);

        let domain = tags
            .remove("d")
            .ok_or_else(|| DkimError::MissingTag("d".into()))?
            .trim()
            .to_string();
        let selector = tags
            .remove("s")
            .ok_or_else(|| DkimError::MissingTag("s".into()))?
            .trim()
            .to_string();

        let h = tags
            .remove("h")
            .ok_or_else(|| DkimError::MissingTag("h".into()))?;
        let signed_headers: Vec<String> = h
            .split(':')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if signed_headers.is_empty() {
            return Err(DkimError::InvalidTag("h= empty".into()));
        }

        // Optional canonicalization tag: c=hdr/body, default simple/simple
        let (canon_header, canon_body) = match tags.remove("c") {
            None => (Canon::Simple, Canon::Simple),
            Some(c) => parse_canon(&c)?,
        };

        // Optional length limit
        let body_length = match tags.remove("l") {
            None => None,
            Some(s) => Some(
                s.trim()
                    .parse()
                    .map_err(|_| DkimError::InvalidTag(format!("l={s}")))?,
            ),
        };

        let timestamp = parse_optional_u64(&mut tags, "t")?;
        let expiration = parse_optional_u64(&mut tags, "x")?;
        let identity = tags.remove("i").map(|s| s.trim().to_string());
        let query_method = tags
            .remove("q")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "dns/txt".to_string());
        if !query_method.eq_ignore_ascii_case("dns/txt") {
            return Err(DkimError::UnsupportedAlgorithm(format!(
                "q={query_method}"
            )));
        }

        Ok(DkimHeader {
            version,
            algorithm,
            signature_b64,
            body_hash_b64,
            canon_header,
            canon_body,
            domain,
            selector,
            signed_headers,
            body_length,
            timestamp,
            expiration,
            identity,
            query_method,
        })
    }
}

fn parse_optional_u64(
    tags: &mut std::collections::HashMap<String, String>,
    name: &str,
) -> Result<Option<u64>, DkimError> {
    match tags.remove(name) {
        None => Ok(None),
        Some(s) => Ok(Some(
            s.trim()
                .parse()
                .map_err(|_| DkimError::InvalidTag(format!("{name}={s}")))?,
        )),
    }
}

fn parse_canon(c: &str) -> Result<(Canon, Canon), DkimError> {
    let c = c.trim();
    // c= can be "header/body" or just "header" (default body = simple)
    let (hdr, body) = match c.split_once('/') {
        Some((h, b)) => (h.trim(), b.trim()),
        None => (c, "simple"),
    };
    let h = match hdr {
        "simple" => Canon::Simple,
        "relaxed" => Canon::Relaxed,
        other => return Err(DkimError::UnsupportedCanon(format!("header={other}"))),
    };
    let b = match body {
        "simple" => Canon::Simple,
        "relaxed" => Canon::Relaxed,
        other => return Err(DkimError::UnsupportedCanon(format!("body={other}"))),
    };
    Ok((h, b))
}

/// RFC 6376 §3.5: tag-list = tag-spec *( ";" tag-spec ).
/// Parse into `HashMap<name, value>` with each tag value as-is (the
/// caller may need to strip WSP per RFC 6376 §3.2 for some tags).
fn parse_tag_list(input: &str) -> Result<std::collections::HashMap<String, String>, DkimError> {
    let mut out = std::collections::HashMap::new();
    for token in input.split(';') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let (name, value) = token
            .split_once('=')
            .ok_or_else(|| DkimError::InvalidTag(format!("no `=` in tag {token:?}")))?;
        let name = name.trim().to_ascii_lowercase();
        out.insert(name, value.to_string());
    }
    Ok(out)
}

/// Remove all WSP (space + horizontal tab) and CR/LF — used for the
/// base64 tag values, which may have arbitrary whitespace inserted by
/// the folding rules.
fn strip_wsp(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
        .collect()
}

/// Unfold a header value: CRLF followed by WSP collapses to a single
/// SP (the WSP itself is preserved by the unfold; RFC 5322 §2.2.3).
fn unfold(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            // Followed by WSP? unfold
            if i + 2 < bytes.len() && matches!(bytes[i + 2], b' ' | b'\t') {
                // Skip the CRLF; keep the WSP
                i += 2;
                continue;
            }
            // CRLF not followed by WSP: stop (end of header)
            break;
        }
        if bytes[i] == b'\n' && i + 1 < bytes.len() && matches!(bytes[i + 1], b' ' | b'\t') {
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal real-world DKIM-Signature (relaxed/relaxed, rsa-sha256).
    fn sample_header() -> &'static str {
        " v=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail;\r\n\
         \th=From:To:Subject:Date:Message-ID;\r\n\
         \tbh=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=;\r\n\
         \tb=SignatureValueGoesHere"
    }

    #[test]
    fn parse_full_header() {
        let h = DkimHeader::parse(sample_header()).unwrap();
        assert_eq!(h.version, 1);
        assert_eq!(h.algorithm, Algorithm::RsaSha256);
        assert_eq!(h.canon_header, Canon::Relaxed);
        assert_eq!(h.canon_body, Canon::Relaxed);
        assert_eq!(h.domain, "example.com");
        assert_eq!(h.selector, "mail");
        assert_eq!(
            h.signed_headers,
            vec!["from", "to", "subject", "date", "message-id"]
        );
        assert_eq!(h.body_hash_b64, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
        assert_eq!(h.signature_b64, "SignatureValueGoesHere");
        assert!(h.body_length.is_none());
        assert_eq!(h.query_method, "dns/txt");
    }

    #[test]
    fn parse_simple_canon_default() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=AAAA; b=BBBB",
        )
        .unwrap();
        assert_eq!(r.canon_header, Canon::Simple);
        assert_eq!(r.canon_body, Canon::Simple);
    }

    #[test]
    fn parse_canon_relaxed_simple() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; c=relaxed/simple; d=e.com; s=s; h=From; bh=A; b=B",
        )
        .unwrap();
        assert_eq!(r.canon_header, Canon::Relaxed);
        assert_eq!(r.canon_body, Canon::Simple);
    }

    #[test]
    fn parse_canon_header_only_defaults_body() {
        // "c=relaxed" without /body part → body defaults to simple
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; c=relaxed; d=e.com; s=s; h=From; bh=A; b=B",
        )
        .unwrap();
        assert_eq!(r.canon_header, Canon::Relaxed);
        assert_eq!(r.canon_body, Canon::Simple);
    }

    #[test]
    fn parse_signed_headers_lowercased() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=From:TO:SuBjEcT; bh=A; b=B",
        )
        .unwrap();
        assert_eq!(r.signed_headers, vec!["from", "to", "subject"]);
    }

    #[test]
    fn parse_optional_l_t_x() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=A; b=B; l=1024; t=1000; x=2000",
        )
        .unwrap();
        assert_eq!(r.body_length, Some(1024));
        assert_eq!(r.timestamp, Some(1000));
        assert_eq!(r.expiration, Some(2000));
    }

    #[test]
    fn parse_rejects_missing_required() {
        // Missing `v=`
        let r = DkimHeader::parse("a=rsa-sha256; d=e.com; s=s; h=From; bh=A; b=B");
        assert!(matches!(r, Err(DkimError::MissingTag(_))));
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let r = DkimHeader::parse(
            "v=2; a=rsa-sha256; d=e.com; s=s; h=From; bh=A; b=B",
        );
        assert!(matches!(r, Err(DkimError::InvalidTag(_))));
    }

    #[test]
    fn parse_rejects_unsupported_algo() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha1; d=e.com; s=s; h=From; bh=A; b=B",
        );
        assert!(matches!(r, Err(DkimError::UnsupportedAlgorithm(_))));
    }

    #[test]
    fn parse_ed25519_sha256_algorithm() {
        // RFC 8463 ed25519-sha256 is accepted in 1.1+
        let r = DkimHeader::parse(
            "v=1; a=ed25519-sha256; d=e.com; s=s; h=From; bh=A; b=B",
        )
        .unwrap();
        assert_eq!(r.algorithm, Algorithm::Ed25519Sha256);
    }

    #[test]
    fn parse_rejects_empty_h() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=; bh=A; b=B",
        );
        assert!(matches!(r, Err(DkimError::InvalidTag(_))));
    }

    #[test]
    fn parse_b_strips_wsp() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=A; b=A B\tC\r\n D",
        )
        .unwrap();
        assert_eq!(r.signature_b64, "ABCD");
    }

    #[test]
    fn parse_default_query_dns_txt() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=A; b=B",
        )
        .unwrap();
        assert_eq!(r.query_method, "dns/txt");
    }

    #[test]
    fn parse_rejects_non_dns_query() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; q=https; d=e.com; s=s; h=From; bh=A; b=B",
        );
        assert!(matches!(r, Err(DkimError::UnsupportedAlgorithm(_))));
    }

    #[test]
    fn parse_with_i_identity() {
        let r = DkimHeader::parse(
            "v=1; a=rsa-sha256; d=e.com; s=s; h=From; bh=A; b=B; i=user@e.com",
        )
        .unwrap();
        assert_eq!(r.identity.as_deref(), Some("user@e.com"));
    }
}
