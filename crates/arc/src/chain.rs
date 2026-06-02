//! ARC chain extraction.
//!
//! Given a raw RFC 5322 message, group every `ARC-Authentication-Results`
//! / `ARC-Message-Signature` / `ARC-Seal` header by its `i=N` instance
//! number to form a per-instance [`ArcSet`], then validate the chain is
//! contiguous from `i=1` upward with no gaps (RFC 8617 §5.2 step 3).

use crate::error::ArcError;
use crate::header::{ArcAuthResults, ArcMessageSignature, ArcSeal, MAX_INSTANCE};

/// One ARC instance's triplet of headers.
#[derive(Debug, Clone)]
pub struct ArcSet {
    /// Instance number (1..=50).
    pub i: u32,
    /// Parsed `ARC-Authentication-Results` header.
    pub aar: ArcAuthResults,
    /// Parsed `ARC-Message-Signature` header.
    pub ams: ArcMessageSignature,
    /// Parsed `ARC-Seal` header.
    pub seal: ArcSeal,
    /// Verbatim header values, in the order they appeared in the
    /// original message. Needed by the seal verifier (it canonicalizes
    /// the prior chain headers as input to its hash).
    pub raw_aar: String,
    /// see [`Self::raw_aar`].
    pub raw_ams: String,
    /// see [`Self::raw_aar`].
    pub raw_seal: String,
}

/// A complete ARC chain extracted from a message.
#[derive(Debug, Clone)]
pub struct ArcChain {
    /// Sets in ascending instance order: `sets[0].i == 1`, etc.
    pub sets: Vec<ArcSet>,
}

impl ArcChain {
    /// Walk the header block of `raw_message` and assemble every
    /// complete ARC instance into an [`ArcSet`]. The header block ends
    /// at the first CRLF CRLF (or LF LF) per RFC 5322 §2.1.
    ///
    /// Returns `Ok(None)` if there are zero ARC headers (the message
    /// is unsigned by any forwarder — DMARC then evaluates as normal).
    ///
    /// Returns `Err(ArcError::IncompleteSet)` if any instance has only
    /// 1 or 2 of the 3 required headers, and
    /// `Err(ArcError::NonContiguousChain)` if instances don't form a
    /// contiguous run starting at 1.
    pub fn extract(raw_message: &[u8]) -> Result<Option<Self>, ArcError> {
        let header_block = take_header_block(raw_message);
        let mut by_instance: std::collections::BTreeMap<u32, PartialSet> =
            std::collections::BTreeMap::new();

        for (name, value) in unfold_headers(header_block) {
            let name_lower = name.to_ascii_lowercase();
            match name_lower.as_str() {
                "arc-authentication-results" => {
                    let aar = ArcAuthResults::parse(&value)?;
                    let i = aar.instance;
                    by_instance.entry(i).or_default().aar = Some((aar, value));
                }
                "arc-message-signature" => {
                    let ams = ArcMessageSignature::parse(&value)?;
                    let i = ams.instance;
                    by_instance.entry(i).or_default().ams = Some((ams, value));
                }
                "arc-seal" => {
                    let seal = ArcSeal::parse(&value)?;
                    let i = seal.instance;
                    by_instance.entry(i).or_default().seal = Some((seal, value));
                }
                _ => {}
            }
        }

        if by_instance.is_empty() {
            return Ok(None);
        }

        // Validate completeness + contiguity. RFC 8617 §5.1 forbids
        // sparse chains. Walk i=1, 2, 3, … and require each set to be
        // complete.
        let mut sets: Vec<ArcSet> = Vec::with_capacity(by_instance.len());
        for expected_i in 1..=MAX_INSTANCE {
            match by_instance.remove(&expected_i) {
                Some(partial) => sets.push(partial.complete(expected_i)?),
                None => {
                    if by_instance.is_empty() {
                        // Reached the end naturally.
                        break;
                    }
                    return Err(ArcError::NonContiguousChain {
                        missing: expected_i,
                    });
                }
            }
        }
        if !by_instance.is_empty() {
            return Err(ArcError::ChainTooLong(sets.len() + by_instance.len()));
        }

        Ok(Some(Self { sets }))
    }

    /// Highest instance number in the chain.
    pub fn highest_instance(&self) -> u32 {
        self.sets.last().map(|s| s.i).unwrap_or(0)
    }
}

#[derive(Default)]
struct PartialSet {
    aar: Option<(ArcAuthResults, String)>,
    ams: Option<(ArcMessageSignature, String)>,
    seal: Option<(ArcSeal, String)>,
}

impl PartialSet {
    fn complete(self, i: u32) -> Result<ArcSet, ArcError> {
        let (aar, raw_aar) = self.aar.ok_or(ArcError::IncompleteSet {
            instance: i,
            missing: "aar",
        })?;
        let (ams, raw_ams) = self.ams.ok_or(ArcError::IncompleteSet {
            instance: i,
            missing: "ams",
        })?;
        let (seal, raw_seal) = self.seal.ok_or(ArcError::IncompleteSet {
            instance: i,
            missing: "seal",
        })?;
        Ok(ArcSet {
            i,
            aar,
            ams,
            seal,
            raw_aar,
            raw_ams,
            raw_seal,
        })
    }
}

/// Extract the header block — everything before the first CRLF CRLF
/// (or LF LF) — and return it as a `&[u8]`. If the separator isn't
/// found, the whole buffer is treated as headers (unusual but legal
/// for a header-only message).
fn take_header_block(raw: &[u8]) -> &[u8] {
    // memchr-anchored body-separator scan. Scans for `\n` and at each
    // candidate checks both `\r\n\r\n` (canonical) and `\n\n`
    // (bare-LF MTAs) shapes in one pass. Replaces the prior two
    // independent O(N·M) `find_subseq` walks (one per shape).
    let mut search = 0;
    while let Some(rel) = memchr::memchr(b'\n', &raw[search..]) {
        let pos = search + rel;
        if pos >= 3 && &raw[pos - 3..=pos] == b"\r\n\r\n" {
            return &raw[..pos - 3];
        }
        if pos >= 1 && raw[pos - 1] == b'\n' {
            return &raw[..pos - 1];
        }
        search = pos + 1;
    }
    raw
}

/// Iterator over `(name, unfolded_value)` headers from a header block.
/// Continuation lines (CRLF + WSP) are joined back into the value, per
/// RFC 5322 §2.2.3.
///
/// memchr-anchored single-pass walk. The previous implementation
/// allocated a `Vec<Vec<u8>>` of line slices up front (one Vec per
/// header line) plus a per-header `Vec<u8>` value buffer with a
/// `Vec::remove(0)` shift-loop to skip leading WSP — O(n²) per
/// continuation line + N+ allocations per call. ARC verify runs
/// this once per inbound message that carries an ARC chain.
fn unfold_headers(block: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < block.len() {
        // Find end-of-line via memchr `\n` (LF) — covers both \r\n and
        // bare-LF. content_end excludes the optional preceding \r.
        let (content_end, after_line) = match memchr::memchr(b'\n', &block[pos..]) {
            Some(off) => {
                let lf = pos + off;
                let ce = if lf > pos && block[lf - 1] == b'\r' {
                    lf - 1
                } else {
                    lf
                };
                (ce, lf + 1)
            }
            None => (block.len(), block.len()),
        };
        let line = &block[pos..content_end];
        pos = after_line;
        if line.is_empty() {
            continue;
        }
        // Find ':' separator via memchr.
        let Some(colon) = memchr::memchr(b':', line) else {
            continue;
        };
        let name = std::str::from_utf8(&line[..colon])
            .unwrap_or_default()
            .trim()
            .to_string();
        // Skip leading WSP after the colon by adjusting the slice
        // pointer — no allocation, no `Vec::remove(0)` shift.
        let mut value_start = colon + 1;
        while value_start < line.len()
            && matches!(line[value_start], b' ' | b'\t')
        {
            value_start += 1;
        }
        // Build the unfolded value. Common case: single-line header,
        // no continuation — just convert the existing slice once.
        let first_segment = &line[value_start..];
        // Lookahead: continuation lines start with WSP. Walk forward
        // and stitch them on.
        let lookahead = &block[pos..];
        if lookahead.is_empty() || !matches!(lookahead.first(), Some(b' ' | b'\t')) {
            // No continuation — fast path, single allocation.
            let value_str = String::from_utf8_lossy(first_segment).into_owned();
            out.push((name, value_str));
            continue;
        }
        let mut value = Vec::with_capacity(first_segment.len() + 64);
        value.extend_from_slice(first_segment);
        while pos < block.len() && matches!(block[pos], b' ' | b'\t') {
            let (cont_end, cont_after) = match memchr::memchr(b'\n', &block[pos..]) {
                Some(off) => {
                    let lf = pos + off;
                    let ce = if lf > pos && block[lf - 1] == b'\r' {
                        lf - 1
                    } else {
                        lf
                    };
                    (ce, lf + 1)
                }
                None => (block.len(), block.len()),
            };
            // Skip leading WSP of the continuation, then append with
            // a single SP separator.
            let mut j = pos;
            while j < cont_end && matches!(block[j], b' ' | b'\t') {
                j += 1;
            }
            value.push(b' ');
            value.extend_from_slice(&block[j..cont_end]);
            pos = cont_after;
        }
        let value_str = String::from_utf8_lossy(&value).into_owned();
        out.push((name, value_str));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AAR1: &str =
        "ARC-Authentication-Results: i=1; spf=pass smtp.mailfrom=alice@example.com\r\n";
    const AMS1: &str = "ARC-Message-Signature: i=1; a=rsa-sha256; c=relaxed/relaxed; d=example.com; s=mail; h=From:To:Subject; bh=BH1; b=SIG1\r\n";
    const AS1: &str = "ARC-Seal: i=1; a=rsa-sha256; cv=none; d=example.com; s=mail; b=SEAL1\r\n";

    const AAR2: &str = "ARC-Authentication-Results: i=2; dkim=pass header.d=forwarder.example\r\n";
    const AMS2: &str = "ARC-Message-Signature: i=2; a=rsa-sha256; c=relaxed/relaxed; d=forwarder.example; s=mail; h=From:To:Subject; bh=BH2; b=SIG2\r\n";
    const AS2: &str =
        "ARC-Seal: i=2; a=rsa-sha256; cv=pass; d=forwarder.example; s=mail; b=SEAL2\r\n";

    fn message_with(headers: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for h in headers {
            out.extend_from_slice(h.as_bytes());
        }
        out.extend_from_slice(b"From: alice@example.com\r\nSubject: t\r\n\r\nbody");
        out
    }

    #[test]
    fn extract_no_arc_returns_none() {
        let msg = b"From: a@b.c\r\nSubject: hi\r\n\r\nbody";
        let chain = ArcChain::extract(msg).unwrap();
        assert!(chain.is_none());
    }

    #[test]
    fn extract_single_set_chain() {
        let msg = message_with(&[AAR1, AMS1, AS1]);
        let chain = ArcChain::extract(&msg).unwrap().unwrap();
        assert_eq!(chain.sets.len(), 1);
        assert_eq!(chain.sets[0].i, 1);
        assert_eq!(chain.sets[0].seal.cv, crate::header::ArcSealCv::None);
    }

    #[test]
    fn extract_two_hop_chain() {
        let msg = message_with(&[AAR1, AMS1, AS1, AAR2, AMS2, AS2]);
        let chain = ArcChain::extract(&msg).unwrap().unwrap();
        assert_eq!(chain.sets.len(), 2);
        assert_eq!(chain.sets[0].i, 1);
        assert_eq!(chain.sets[1].i, 2);
        assert_eq!(chain.sets[1].seal.cv, crate::header::ArcSealCv::Pass);
    }

    #[test]
    fn extract_header_order_independent() {
        // Even if a forwarder put i=2 headers before i=1 (unusual but
        // legal), extraction must succeed and order them by instance.
        let msg = message_with(&[AAR2, AMS2, AS2, AAR1, AMS1, AS1]);
        let chain = ArcChain::extract(&msg).unwrap().unwrap();
        assert_eq!(chain.sets.len(), 2);
        assert_eq!(chain.sets[0].i, 1);
        assert_eq!(chain.sets[1].i, 2);
    }

    #[test]
    fn extract_rejects_incomplete_set() {
        // i=1 missing the seal.
        let msg = message_with(&[AAR1, AMS1]);
        let r = ArcChain::extract(&msg);
        assert!(matches!(
            r,
            Err(ArcError::IncompleteSet {
                instance: 1,
                missing: "seal"
            })
        ));
    }

    #[test]
    fn extract_rejects_non_contiguous_chain() {
        // i=1 + i=3 with i=2 missing entirely.
        const AAR3: &str = "ARC-Authentication-Results: i=3; dkim=pass\r\n";
        const AMS3: &str = "ARC-Message-Signature: i=3; a=rsa-sha256; d=x.example; s=mail; h=From; bh=BH3; b=SIG3\r\n";
        const AS3: &str = "ARC-Seal: i=3; a=rsa-sha256; cv=pass; d=x.example; s=mail; b=SEAL3\r\n";
        let msg = message_with(&[AAR1, AMS1, AS1, AAR3, AMS3, AS3]);
        let r = ArcChain::extract(&msg);
        assert!(matches!(
            r,
            Err(ArcError::NonContiguousChain { missing: 2 })
        ));
    }

    #[test]
    fn extract_handles_folded_headers() {
        let folded = "ARC-Message-Signature: i=1; a=rsa-sha256;\r\n c=relaxed/relaxed;\r\n d=example.com;\r\n s=mail; h=From:To:Subject; bh=BH1; b=SIG1\r\n";
        let msg = {
            let mut v = Vec::new();
            v.extend_from_slice(AAR1.as_bytes());
            v.extend_from_slice(folded.as_bytes());
            v.extend_from_slice(AS1.as_bytes());
            v.extend_from_slice(b"From: alice@example.com\r\n\r\nbody");
            v
        };
        let chain = ArcChain::extract(&msg).unwrap().unwrap();
        assert_eq!(chain.sets.len(), 1);
        assert_eq!(chain.sets[0].ams.canon_body, crate::header::Canon::Relaxed);
    }

    #[test]
    fn highest_instance_returns_last() {
        let msg = message_with(&[AAR1, AMS1, AS1, AAR2, AMS2, AS2]);
        let chain = ArcChain::extract(&msg).unwrap().unwrap();
        assert_eq!(chain.highest_instance(), 2);
    }

    #[test]
    fn extract_preserves_raw_values() {
        let msg = message_with(&[AAR1, AMS1, AS1]);
        let chain = ArcChain::extract(&msg).unwrap().unwrap();
        let set = &chain.sets[0];
        assert!(set.raw_aar.contains("spf=pass"));
        assert!(set.raw_ams.contains("BH1"));
        assert!(set.raw_seal.contains("SEAL1"));
    }
}
