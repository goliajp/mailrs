//! ARC header parsers (RFC 8617 §4.1).
//!
//! Three header types, all using the same `;`-separated tag-list syntax
//! as DKIM-Signature (RFC 6376 §3.2):
//!
//! - **ARC-Authentication-Results** (AAR): `i=N; <authres-body>`
//!   The body after `i=N;` is verbatim `Authentication-Results` syntax
//!   (RFC 8601 §2.2). This crate keeps the body as an unparsed string —
//!   downstream readers / DMARC layers walk it.
//! - **ARC-Message-Signature** (AMS): DKIM-Signature shape plus a
//!   required `i=N` (instance) tag.
//! - **ARC-Seal** (AS): smaller set of tags — `i`, `a`, `b`, `cv`,
//!   `d`, `s`, `t`. Does NOT carry `h=` or `bh=` because the seal
//!   signs the chain (preceding ARC headers), not the message body.
//!
//! All three parsers are single-pass byte scanners with byte-literal
//! tag dispatch — same shape as `mailrs_dkim::DkimHeader::parse` so
//! the parse cost is sub-µs on realistic headers.

use crate::error::ArcError;

/// Maximum legal instance number per RFC 8617 §4.2.1.
pub const MAX_INSTANCE: u32 = 50;

/// `cv=` chain-validation status on `ARC-Seal`. RFC 8617 §4.1.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcSealCv {
    /// First hop in the chain (no prior chain to validate).
    None,
    /// Prior chain validated successfully.
    Pass,
    /// Prior chain validation failed.
    Fail,
}

impl ArcSealCv {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            _ => None,
        }
    }
}

/// Parsed `ARC-Authentication-Results` (AAR) header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcAuthResults {
    /// Instance number (1..=50).
    pub instance: u32,
    /// Verbatim authres body — everything after `i=N;`. Use a
    /// `mailrs_inbound`-style `Authentication-Results` parser or
    /// equivalent to walk it; this crate doesn't parse the body
    /// further because the body shape is open-ended (any number of
    /// `method.result` clauses + optional reason / properties).
    pub authres: String,
}

impl ArcAuthResults {
    /// Parse an `ARC-Authentication-Results` header value.
    pub fn parse(value: &str) -> Result<Self, ArcError> {
        let (instance, rest) = take_instance(value)?;
        // The remainder after `i=N;` is the verbatim authres body.
        // Trim leading whitespace and trailing whitespace.
        let authres = rest.trim().to_string();
        Ok(Self { instance, authres })
    }
}

/// Parsed `ARC-Message-Signature` (AMS) header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcMessageSignature {
    /// `i=` instance number (1..=50).
    pub instance: u32,
    /// `a=` algorithm — `rsa-sha256` or `ed25519-sha256`.
    pub algorithm: Algorithm,
    /// `b=` base64 signature bytes.
    pub signature_b64: String,
    /// `bh=` base64 body-hash bytes.
    pub body_hash_b64: String,
    /// `c=` canonicalization: `(header, body)` modes.
    pub canon_header: Canon,
    /// see [`Self::canon_header`].
    pub canon_body: Canon,
    /// `d=` signing domain.
    pub domain: String,
    /// `s=` selector.
    pub selector: String,
    /// `h=` colon-separated list of signed header names (lowercased).
    pub signed_headers: Vec<String>,
    /// `t=` timestamp (epoch seconds), optional.
    pub timestamp: Option<u64>,
    /// `x=` expiration (epoch seconds), optional.
    pub expiration: Option<u64>,
}

impl ArcMessageSignature {
    /// Parse an `ARC-Message-Signature` header value.
    pub fn parse(value: &str) -> Result<Self, ArcError> {
        let mut instance: Option<u32> = None;
        let mut algorithm: Option<Algorithm> = None;
        let mut signature_b64: Option<String> = None;
        let mut body_hash_b64: Option<String> = None;
        let mut canon_header = Canon::Simple;
        let mut canon_body = Canon::Simple;
        let mut domain: Option<String> = None;
        let mut selector: Option<String> = None;
        let mut signed_headers: Option<Vec<String>> = None;
        let mut timestamp: Option<u64> = None;
        let mut expiration: Option<u64> = None;

        for (name, raw_val) in TagIter::new(value) {
            match name.as_bytes() {
                b"i" => instance = Some(parse_instance_value(raw_val)?),
                b"a" => algorithm = Some(parse_algorithm(raw_val)?),
                b"b" => signature_b64 = Some(strip_wsp(raw_val)),
                b"bh" => body_hash_b64 = Some(strip_wsp(raw_val)),
                b"c" => {
                    let (h, b) = parse_canon(raw_val)?;
                    canon_header = h;
                    canon_body = b;
                }
                b"d" => domain = Some(raw_val.trim().to_ascii_lowercase()),
                b"s" => selector = Some(raw_val.trim().to_ascii_lowercase()),
                b"h" => {
                    let mut list: Vec<String> = Vec::with_capacity(8);
                    let mut cur: Vec<u8> = Vec::with_capacity(20);
                    for &c in raw_val.as_bytes() {
                        match c {
                            b' ' | b'\t' | b'\r' | b'\n' => {}
                            b':' => {
                                if !cur.is_empty() {
                                    // SAFETY: only lowercase ASCII pushed below.
                                    let s = unsafe {
                                        String::from_utf8_unchecked(std::mem::take(&mut cur))
                                    };
                                    list.push(s);
                                    cur.reserve(20);
                                }
                            }
                            _ => cur.push(c.to_ascii_lowercase()),
                        }
                    }
                    if !cur.is_empty() {
                        // SAFETY: only lowercase ASCII pushed.
                        let s = unsafe { String::from_utf8_unchecked(cur) };
                        list.push(s);
                    }
                    if list.is_empty() {
                        return Err(ArcError::InvalidTag("h= empty".into()));
                    }
                    signed_headers = Some(list);
                }
                b"t" => timestamp = Some(parse_u64(raw_val, "t")?),
                b"x" => expiration = Some(parse_u64(raw_val, "x")?),
                // Unknown tags are ignored per RFC 6376 §3.2 (and ARC
                // inherits the syntax). Same forward-compat rule.
                _ => {}
            }
        }

        Ok(Self {
            instance: instance.ok_or_else(|| ArcError::MissingTag("i".into()))?,
            algorithm: algorithm.ok_or_else(|| ArcError::MissingTag("a".into()))?,
            signature_b64: signature_b64.ok_or_else(|| ArcError::MissingTag("b".into()))?,
            body_hash_b64: body_hash_b64.ok_or_else(|| ArcError::MissingTag("bh".into()))?,
            canon_header,
            canon_body,
            domain: domain.ok_or_else(|| ArcError::MissingTag("d".into()))?,
            selector: selector.ok_or_else(|| ArcError::MissingTag("s".into()))?,
            signed_headers: signed_headers.ok_or_else(|| ArcError::MissingTag("h".into()))?,
            timestamp,
            expiration,
        })
    }
}

/// Parsed `ARC-Seal` (AS) header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcSeal {
    /// `i=` instance number (1..=50).
    pub instance: u32,
    /// `a=` algorithm.
    pub algorithm: Algorithm,
    /// `b=` base64 seal signature bytes.
    pub signature_b64: String,
    /// `cv=` chain-validation status. `None` only for `i=1`.
    pub cv: ArcSealCv,
    /// `d=` signing domain.
    pub domain: String,
    /// `s=` selector.
    pub selector: String,
    /// `t=` timestamp (epoch seconds), optional.
    pub timestamp: Option<u64>,
}

impl ArcSeal {
    /// Parse an `ARC-Seal` header value.
    pub fn parse(value: &str) -> Result<Self, ArcError> {
        let mut instance: Option<u32> = None;
        let mut algorithm: Option<Algorithm> = None;
        let mut signature_b64: Option<String> = None;
        let mut cv: Option<ArcSealCv> = None;
        let mut domain: Option<String> = None;
        let mut selector: Option<String> = None;
        let mut timestamp: Option<u64> = None;

        for (name, raw_val) in TagIter::new(value) {
            match name.as_bytes() {
                b"i" => instance = Some(parse_instance_value(raw_val)?),
                b"a" => algorithm = Some(parse_algorithm(raw_val)?),
                b"b" => signature_b64 = Some(strip_wsp(raw_val)),
                b"cv" => {
                    cv = Some(
                        ArcSealCv::parse(raw_val)
                            .ok_or_else(|| ArcError::InvalidCv(raw_val.trim().into()))?,
                    );
                }
                b"d" => domain = Some(raw_val.trim().to_ascii_lowercase()),
                b"s" => selector = Some(raw_val.trim().to_ascii_lowercase()),
                b"t" => timestamp = Some(parse_u64(raw_val, "t")?),
                _ => {}
            }
        }

        Ok(Self {
            instance: instance.ok_or_else(|| ArcError::MissingTag("i".into()))?,
            algorithm: algorithm.ok_or_else(|| ArcError::MissingTag("a".into()))?,
            signature_b64: signature_b64.ok_or_else(|| ArcError::MissingTag("b".into()))?,
            cv: cv.ok_or_else(|| ArcError::MissingTag("cv".into()))?,
            domain: domain.ok_or_else(|| ArcError::MissingTag("d".into()))?,
            selector: selector.ok_or_else(|| ArcError::MissingTag("s".into()))?,
            timestamp,
        })
    }
}

/// Algorithm announced in the `a=` tag of an AMS or AS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// `a=rsa-sha256` — RSA over SHA-256.
    RsaSha256,
    /// `a=ed25519-sha256` — Ed25519 over SHA-256, per RFC 8463.
    Ed25519Sha256,
}

/// Canonicalization mode (mirrors `mailrs_dkim::Canon`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canon {
    /// `simple`: body must end with one CRLF, ignore trailing empty
    /// lines; headers untouched.
    Simple,
    /// `relaxed`: body collapses internal WSP; headers lowercased + WSP
    /// collapsed.
    Relaxed,
}

// ---------- internal helpers ----------

fn parse_instance_value(s: &str) -> Result<u32, ArcError> {
    let n: u32 = s
        .trim()
        .parse()
        .map_err(|_| ArcError::InvalidTag(format!("i={}", s.trim())))?;
    if !(1..=MAX_INSTANCE).contains(&n) {
        return Err(ArcError::InvalidInstance(n));
    }
    Ok(n)
}

fn parse_algorithm(s: &str) -> Result<Algorithm, ArcError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "rsa-sha256" => Ok(Algorithm::RsaSha256),
        "ed25519-sha256" => Ok(Algorithm::Ed25519Sha256),
        other => Err(ArcError::UnsupportedAlgorithm(other.to_string())),
    }
}

fn parse_canon(s: &str) -> Result<(Canon, Canon), ArcError> {
    let s = s.trim();
    let (h, b) = match s.split_once('/') {
        Some((h, b)) => (h.trim(), b.trim()),
        None => (s, "simple"),
    };
    let h = match h {
        "simple" => Canon::Simple,
        "relaxed" => Canon::Relaxed,
        other => return Err(ArcError::InvalidTag(format!("c header={other}"))),
    };
    let b = match b {
        "simple" => Canon::Simple,
        "relaxed" => Canon::Relaxed,
        other => return Err(ArcError::InvalidTag(format!("c body={other}"))),
    };
    Ok((h, b))
}

fn parse_u64(s: &str, name: &str) -> Result<u64, ArcError> {
    s.trim()
        .parse()
        .map_err(|_| ArcError::InvalidTag(format!("{name}={}", s.trim())))
}

fn strip_wsp(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if !matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
            out.push(b as char);
        }
    }
    out
}

/// Take the mandatory `i=N` prefix off an AAR header. Returns
/// `(instance, rest)` where `rest` is everything after the first `;`.
fn take_instance(value: &str) -> Result<(u32, &str), ArcError> {
    // AAR shape: `i=N; <authres-body>` — find first `;`.
    let bytes = value.as_bytes();
    let mut i = 0usize;
    let n = bytes.len();
    // Skip leading WSP.
    while i < n && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    // Expect "i" (case-insensitive).
    if i >= n || !bytes[i].eq_ignore_ascii_case(&b'i') {
        return Err(ArcError::MissingTag("i".into()));
    }
    i += 1;
    while i < n && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if i >= n || bytes[i] != b'=' {
        return Err(ArcError::MissingTag("i (no '=')".into()));
    }
    i += 1;
    let num_start = i;
    while i < n && bytes[i] != b';' {
        i += 1;
    }
    let num = value[num_start..i].trim();
    let instance: u32 = num
        .parse()
        .map_err(|_| ArcError::InvalidTag(format!("i={num}")))?;
    if !(1..=MAX_INSTANCE).contains(&instance) {
        return Err(ArcError::InvalidInstance(instance));
    }
    let rest = if i < n { &value[i + 1..] } else { "" };
    Ok((instance, rest))
}

/// Iterator over `(name, value)` tag pairs in a DKIM-style tag list.
/// Tag names are returned lowercased; values are NOT trimmed (callers
/// trim or strip-WSP as appropriate per tag).
struct TagIter<'a> {
    bytes: &'a [u8],
    source: &'a str,
    i: usize,
}

impl<'a> TagIter<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            source,
            i: 0,
        }
    }
}

impl<'a> Iterator for TagIter<'a> {
    type Item = (String, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let n = self.bytes.len();
        // Skip separators / wsp / folding.
        while self.i < n && matches!(self.bytes[self.i], b' ' | b'\t' | b'\r' | b'\n' | b';') {
            self.i += 1;
        }
        if self.i >= n {
            return None;
        }
        // Tag name.
        let name_start = self.i;
        while self.i < n
            && !matches!(
                self.bytes[self.i],
                b'=' | b' ' | b'\t' | b'\r' | b'\n' | b';'
            )
        {
            self.i += 1;
        }
        let name_bytes = &self.bytes[name_start..self.i];
        if name_bytes.is_empty() {
            return None;
        }
        // Optional WSP before '='.
        while self.i < n && matches!(self.bytes[self.i], b' ' | b'\t') {
            self.i += 1;
        }
        if self.i >= n || self.bytes[self.i] != b'=' {
            // Malformed tag at end of input — give up.
            return None;
        }
        self.i += 1;
        let val_start = self.i;
        while self.i < n && self.bytes[self.i] != b';' {
            self.i += 1;
        }
        let val_slice = &self.source[val_start..self.i];
        let name = std::str::from_utf8(name_bytes)
            .unwrap_or_default()
            .to_ascii_lowercase();
        Some((name, val_slice))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aar_parse_minimal() {
        let v = "i=1; spf=pass smtp.mailfrom=alice@example.com";
        let aar = ArcAuthResults::parse(v).unwrap();
        assert_eq!(aar.instance, 1);
        assert_eq!(aar.authres, "spf=pass smtp.mailfrom=alice@example.com");
    }

    #[test]
    fn aar_parse_multi_method() {
        let v = "i=2; spf=pass; dkim=pass header.d=example.com; dmarc=pass";
        let aar = ArcAuthResults::parse(v).unwrap();
        assert_eq!(aar.instance, 2);
        assert!(aar.authres.contains("dkim=pass"));
        assert!(aar.authres.contains("dmarc=pass"));
    }

    #[test]
    fn aar_rejects_missing_i() {
        let r = ArcAuthResults::parse("spf=pass smtp.mailfrom=alice@example.com");
        assert!(matches!(r, Err(ArcError::MissingTag(_))));
    }

    #[test]
    fn aar_rejects_instance_zero() {
        let r = ArcAuthResults::parse("i=0; spf=pass");
        assert!(matches!(r, Err(ArcError::InvalidInstance(0))));
    }

    #[test]
    fn aar_rejects_instance_over_50() {
        let r = ArcAuthResults::parse("i=51; spf=pass");
        assert!(matches!(r, Err(ArcError::InvalidInstance(51))));
    }

    #[test]
    fn ams_parse_minimal() {
        let v = "i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; \
                 s=mail; h=From:To:Subject; bh=BASE64BH; b=BASE64SIG";
        let ams = ArcMessageSignature::parse(v).unwrap();
        assert_eq!(ams.instance, 1);
        assert_eq!(ams.algorithm, Algorithm::RsaSha256);
        assert_eq!(ams.canon_header, Canon::Relaxed);
        assert_eq!(ams.canon_body, Canon::Relaxed);
        assert_eq!(ams.domain, "example.com");
        assert_eq!(ams.selector, "mail");
        assert_eq!(ams.signed_headers, vec!["from", "to", "subject"]);
        assert_eq!(ams.body_hash_b64, "BASE64BH");
        assert_eq!(ams.signature_b64, "BASE64SIG");
        assert_eq!(ams.timestamp, None);
    }

    #[test]
    fn ams_parse_with_timestamp_and_expiry() {
        let v = "i=1; a=rsa-sha256; d=example.com; s=mail; \
                 h=From; bh=X; b=Y; t=1700000000; x=1700003600";
        let ams = ArcMessageSignature::parse(v).unwrap();
        assert_eq!(ams.timestamp, Some(1_700_000_000));
        assert_eq!(ams.expiration, Some(1_700_003_600));
    }

    #[test]
    fn ams_parse_ed25519() {
        let v = "i=1; a=ed25519-sha256; d=example.com; s=mail; \
                 h=From; bh=X; b=Y";
        let ams = ArcMessageSignature::parse(v).unwrap();
        assert_eq!(ams.algorithm, Algorithm::Ed25519Sha256);
    }

    #[test]
    fn ams_rejects_missing_required() {
        // missing b=
        let r =
            ArcMessageSignature::parse("i=1; a=rsa-sha256; d=example.com; s=mail; h=From; bh=X");
        assert!(matches!(r, Err(ArcError::MissingTag(_))));
    }

    #[test]
    fn ams_rejects_unknown_algorithm() {
        let r = ArcMessageSignature::parse(
            "i=1; a=md5-cleartext; d=example.com; s=mail; h=From; bh=X; b=Y",
        );
        assert!(matches!(r, Err(ArcError::UnsupportedAlgorithm(_))));
    }

    #[test]
    fn ams_rejects_empty_h() {
        let r =
            ArcMessageSignature::parse("i=1; a=rsa-sha256; d=example.com; s=mail; h=; bh=X; b=Y");
        assert!(matches!(r, Err(ArcError::InvalidTag(_))));
    }

    #[test]
    fn as_parse_first_hop() {
        let v = "i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; b=BASE64";
        let seal = ArcSeal::parse(v).unwrap();
        assert_eq!(seal.instance, 1);
        assert_eq!(seal.cv, ArcSealCv::None);
        assert_eq!(seal.signature_b64, "BASE64");
    }

    #[test]
    fn as_parse_cv_pass() {
        let v = "i=2; a=rsa-sha256; cv=pass; d=forwarder.example; s=mail; b=BASE64";
        let seal = ArcSeal::parse(v).unwrap();
        assert_eq!(seal.cv, ArcSealCv::Pass);
        assert_eq!(seal.domain, "forwarder.example");
    }

    #[test]
    fn as_parse_cv_fail() {
        let v = "i=3; a=rsa-sha256; cv=fail; d=mx.example; s=mail; b=BASE64";
        let seal = ArcSeal::parse(v).unwrap();
        assert_eq!(seal.cv, ArcSealCv::Fail);
    }

    #[test]
    fn as_rejects_invalid_cv() {
        let r = ArcSeal::parse("i=1; a=rsa-sha256; cv=maybe; d=x; s=y; b=Z");
        assert!(matches!(r, Err(ArcError::InvalidCv(_))));
    }

    #[test]
    fn as_rejects_missing_cv() {
        let r = ArcSeal::parse("i=1; a=rsa-sha256; d=x; s=y; b=Z");
        assert!(matches!(r, Err(ArcError::MissingTag(_))));
    }

    #[test]
    fn ams_strip_wsp_in_b_and_bh() {
        let v = "i=1; a=rsa-sha256; d=example.com; s=mail; h=From; \
                 bh=ABCD EFGH\r\n IJKL; b=WXYZ\r\n 1234\t5678";
        let ams = ArcMessageSignature::parse(v).unwrap();
        assert_eq!(ams.body_hash_b64, "ABCDEFGHIJKL");
        assert_eq!(ams.signature_b64, "WXYZ12345678");
    }

    #[test]
    fn tag_iter_basic() {
        let v = "i=1; a=rsa-sha256; b=BASE";
        let pairs: Vec<_> = TagIter::new(v).collect();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0, "i");
        assert_eq!(pairs[0].1, "1");
        assert_eq!(pairs[1].0, "a");
        assert_eq!(pairs[1].1, "rsa-sha256");
        assert_eq!(pairs[2].0, "b");
        assert_eq!(pairs[2].1, "BASE");
    }
}
