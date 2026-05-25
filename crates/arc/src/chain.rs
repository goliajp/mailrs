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
    // Look for CRLF CRLF first; fall back to LF LF.
    if let Some(pos) = find_subseq(raw, b"\r\n\r\n") {
        &raw[..pos]
    } else if let Some(pos) = find_subseq(raw, b"\n\n") {
        &raw[..pos]
    } else {
        raw
    }
}

fn find_subseq(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    for i in 0..=(hay.len() - needle.len()) {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

/// Iterator over `(name, unfolded_value)` headers from a header block.
/// Continuation lines (CRLF + WSP) are joined back into the value, per
/// RFC 5322 §2.2.3.
fn unfold_headers(block: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();

    for &b in block {
        if b == b'\n' {
            // Trim trailing \r if present.
            if cur.last() == Some(&b'\r') {
                cur.pop();
            }
            lines.push(std::mem::take(&mut cur));
        } else {
            cur.push(b);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }

    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
        if line.is_empty() {
            i += 1;
            continue;
        }
        // Find ':' separating name from value.
        let Some(colon) = line.iter().position(|&c| c == b':') else {
            i += 1;
            continue;
        };
        let name = std::str::from_utf8(&line[..colon])
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut value: Vec<u8> = line[colon + 1..].to_vec();
        // Trim leading WSP after the colon — RFC 5322 says exactly one
        // SP is canonical but in the wild it's "any amount".
        while value
            .first()
            .map(|b| matches!(b, b' ' | b'\t'))
            .unwrap_or(false)
        {
            value.remove(0);
        }
        // Pull in continuation lines.
        i += 1;
        while i < lines.len()
            && lines[i]
                .first()
                .map(|b| matches!(b, b' ' | b'\t'))
                .unwrap_or(false)
        {
            value.push(b' ');
            // Skip leading WSP of the continuation line, then append.
            let mut j = 0;
            while j < lines[i].len() && matches!(lines[i][j], b' ' | b'\t') {
                j += 1;
            }
            value.extend_from_slice(&lines[i][j..]);
            i += 1;
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
