//! The domains a message's own headers put their name to: the `From:`,
//! `Sender:` and `Return-Path:` addresses, the `Message-ID:` host, and
//! each DKIM signature's `d=`.
//!
//! What a link is compared against when the question is whether it goes
//! somewhere the message never otherwise names. Read as written: nothing
//! here says any of them was verified, only that the message claims them,
//! which is what a link to an unrelated domain fails to match.

const HEAD_LIMIT: usize = 64 * 1024;

/// The hosts, lowercased, in no particular order, duplicates kept.
pub fn hosts(raw: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, value) in fields(raw) {
        let host = match name.as_str() {
            "from" | "sender" | "return-path" | "message-id" => after_at(&value),
            _ => None,
        };
        out.extend(host);
    }
    out.extend(
        crate::dkim_signers::of(raw)
            .into_iter()
            .map(|d| d.to_ascii_lowercase()),
    );
    out
}

/// The part after the last `@`, up to the closing `>` or the end.
fn after_at(value: &str) -> Option<String> {
    let (_, rest) = value.rsplit_once('@')?;
    let host = rest
        .split(|c: char| c == '>' || c.is_whitespace() || c == ';' || c == ',')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Header fields, unfolded, names lowercased.
fn fields(raw: &[u8]) -> Vec<(String, String)> {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = out.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::hosts;

    #[test]
    fn the_headers_that_name_a_domain_are_read() {
        let raw = b"From: Billy <billy@jenkins.example>\r\nReturn-Path: <bounce@mail.zoho.example>\r\nMessage-ID: <1.2@smtppro.zoho.example>\r\nDKIM-Signature: v=1; a=rsa-sha256; d=jenkins.example;\r\n s=zoho; b=x\r\nSubject: hi\r\n\r\nbody@not.read.example";
        let mut got = hosts(raw);
        got.sort();
        got.dedup();
        assert_eq!(
            got,
            [
                "jenkins.example",
                "mail.zoho.example",
                "smtppro.zoho.example"
            ]
        );
    }

    #[test]
    fn a_message_without_them_names_nothing() {
        assert!(hosts(b"Subject: hi\r\n\r\nfrom: a@b.example").is_empty());
    }
}
