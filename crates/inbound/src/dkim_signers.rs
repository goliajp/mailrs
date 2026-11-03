//! The `d=` of every `DKIM-Signature:` on a message.
//!
//! The receiver records one DKIM result for all signatures and not who
//! signed, so the signers are read back off the message itself, the same
//! way on the receive path, the stored re-judge and the offline compare.

/// Header bytes searched; signatures sit above `From:` and carry long
/// `b=` values, so this is wider than the identity headers need.
const HEAD_LIMIT: usize = 64 * 1024;

/// The lowercased `d=` domain of each `DKIM-Signature:` header, in order.
/// A signature without a readable `d=` is skipped.
#[must_use]
pub fn of(raw: &[u8]) -> Vec<String> {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut fields: Vec<String> = Vec::new();
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(last) = fields.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
            continue;
        }
        fields.push(line.to_string());
    }
    fields
        .iter()
        .filter_map(|f| f.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("dkim-signature"))
        .filter_map(|(_, value)| domain_tag(value))
        .collect()
}

fn domain_tag(value: &str) -> Option<String> {
    value.split(';').find_map(|tag| {
        let (name, v) = tag.split_once('=')?;
        let d = v
            .split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase();
        (name.trim().eq_ignore_ascii_case("d") && !d.is_empty()).then_some(d)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_signature_is_read_including_folded_ones() {
        let raw =
            b"DKIM-Signature: v=1; a=rsa-sha256;\r\n\td=Mail.Example.COM; s=s1; bh=x; b=y\r\n\
DKIM-Signature: v=1; d=esp.example; s=k; b=z\r\n\
From: a@mail.example.com\r\n\r\nbody d=evil.example\r\n";
        assert_eq!(of(raw), vec!["mail.example.com", "esp.example"]);
    }

    #[test]
    fn no_signature_is_no_signer() {
        assert!(of(b"From: a@b.example\r\n\r\nDKIM-Signature: d=x.example\r\n").is_empty());
        assert!(of(b"DKIM-Signature: v=1; s=s1; b=y\r\n\r\n").is_empty());
    }
}
