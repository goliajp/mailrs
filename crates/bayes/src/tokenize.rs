//! RFC 5322 bytes → feature tokens.
//!
//! Graham's method: each token counts once per message (dedup), so the
//! output is a deduplicated set. Feature tokens carry a namespace prefix
//! (`sub:` / `from:` / `url:` / `hdr:`) so a word in the Subject scores
//! independently from the same word in the body — Subject words carry
//! more discriminatory weight in practice.

use std::collections::BTreeSet;

/// Max distinct tokens emitted per message — bounds a pathological
/// giant message from flooding the corpus.
const MAX_TOKENS: usize = 500;

/// Split raw RFC 5322 message bytes into a deduplicated feature-token
/// set. Lossy UTF-8 decode (mail is frequently not clean UTF-8);
/// non-decodable bytes become replacement chars and tokenize to
/// nothing, which is fine.
pub fn tokenize(raw: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(raw);
    let (headers, body) = split_headers_body(&text);

    let mut out: BTreeSet<String> = BTreeSet::new();

    // Header-derived feature tokens.
    for (name, value) in parse_headers(headers) {
        match name.as_str() {
            "subject" => {
                for w in words(&value) {
                    out.insert(format!("sub:{w}"));
                }
            }
            "from" => {
                if let Some(dom) = sender_domain(&value) {
                    out.insert(format!("from:{dom}"));
                }
                // v2.9 triage — automated-sender signal (noreply@ /
                // bounce@ / notification@ / mailer-daemon@ local-parts).
                // A strong Notifications discriminator.
                if is_automated_sender(&value) {
                    out.insert("from:automated".to_string());
                }
            }
            // v2.9 triage header signals — crisp Notifications /
            // Promotions discriminators the body-token stats miss.
            "list-unsubscribe" => {
                out.insert("hdr:list-unsub".to_string());
            }
            "list-id" => {
                out.insert("hdr:list-id".to_string());
            }
            "precedence" => {
                let v = value.trim().to_lowercase();
                if !v.is_empty() {
                    out.insert(format!("hdr:precedence:{v}"));
                }
            }
            "auto-submitted" => {
                // Any value other than "no" marks automated mail.
                if !value.trim().eq_ignore_ascii_case("no") {
                    out.insert("hdr:auto-submitted".to_string());
                }
            }
            "content-type" => {
                let lc = value.to_lowercase();
                if let Some(main) = lc.split(';').next() {
                    let main = main.trim();
                    if !main.is_empty() {
                        out.insert(format!("hdr:ct:{main}"));
                    }
                }
                if let Some(cs) = extract_charset(&lc) {
                    out.insert(format!("hdr:charset:{cs}"));
                }
            }
            _ => {}
        }
    }

    // Body words + URL-domain feature tokens.
    let body_text = strip_html(body);
    for w in words(&body_text) {
        out.insert(w);
    }
    for dom in url_domains(body) {
        out.insert(format!("url:{dom}"));
    }

    out.into_iter().take(MAX_TOKENS).collect()
}

/// Split on the first blank line (CRLF CRLF or LF LF). Everything
/// before is headers, after is body. A message with no blank line is
/// all-headers (degenerate, but tokenizes fine).
fn split_headers_body(text: &str) -> (&str, &str) {
    if let Some(i) = text.find("\r\n\r\n") {
        (&text[..i], &text[i + 4..])
    } else if let Some(i) = text.find("\n\n") {
        (&text[..i], &text[i + 2..])
    } else {
        (text, "")
    }
}

/// Parse folded headers into `(lowercased-name, value)` pairs. Handles
/// RFC 5322 folding (continuation lines start with whitespace).
fn parse_headers(headers: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for raw_line in headers.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            // Continuation of the previous header value.
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_lowercase(), value.trim().to_string()));
        }
    }
    out
}

/// Extract lowercased ASCII word tokens, length 3..=20, alnum + a few
/// spam-signal punctuation (`'`, `$`, `-`). CJK runs (no word breaks)
/// are bigram-split so Chinese/Japanese spam still yields tokens.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cjk_run: Vec<char> = Vec::new();

    let flush_word = |cur: &mut String, out: &mut Vec<String>| {
        if cur.len() >= 3 && cur.len() <= 20 {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    let flush_cjk = |run: &mut Vec<char>, out: &mut Vec<String>| {
        if run.len() == 1 {
            out.push(run[0].to_string());
        } else {
            for pair in run.windows(2) {
                out.push(pair.iter().collect());
            }
        }
        run.clear();
    };

    for ch in text.chars() {
        if is_cjk(ch) {
            flush_word(&mut cur, &mut out);
            cjk_run.push(ch);
            continue;
        }
        if !cjk_run.is_empty() {
            flush_cjk(&mut cjk_run, &mut out);
        }
        let lc = ch.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() || matches!(lc, '\'' | '$' | '-') {
            cur.push(lc);
        } else {
            flush_word(&mut cur, &mut out);
        }
    }
    flush_word(&mut cur, &mut out);
    if !cjk_run.is_empty() {
        flush_cjk(&mut cjk_run, &mut out);
    }
    out
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x4E00..=0x9FFF   // CJK Unified
        | 0x3040..=0x30FF // Hiragana + Katakana
        | 0xAC00..=0xD7AF // Hangul
    )
}

/// Strip HTML tags, returning visible text. Naive but adequate for
/// tokenizing — we only need words, not a DOM.
fn strip_html(body: &str) -> String {
    if !body.contains('<') {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut in_tag = false;
    for ch in body.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Lowercased registrable-ish domain from a `From:` value — the part
/// after `@`, trimmed of angle brackets / trailing punctuation.
fn sender_domain(value: &str) -> Option<String> {
    let at = value.rfind('@')?;
    let tail = &value[at + 1..];
    let dom: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        .collect::<String>()
        .to_lowercase();
    if dom.contains('.') { Some(dom) } else { None }
}

/// True if the From address' local-part looks like an automated /
/// transactional sender (noreply / no-reply / bounce / notification /
/// mailer-daemon / donotreply). A strong Notifications signal, mirroring
/// `mailrs_clean::sender::is_automated_sender`.
fn is_automated_sender(value: &str) -> bool {
    let at = match value.rfind('@') {
        Some(i) => i,
        None => return false,
    };
    // Local part = the token right before the '@' (strip display name).
    let head = &value[..at];
    let local = head
        .rsplit(['<', ' ', '"'])
        .next()
        .unwrap_or(head)
        .to_lowercase();
    const PATTERNS: [&str; 7] = [
        "noreply",
        "no-reply",
        "donotreply",
        "do-not-reply",
        "bounce",
        "notification",
        "mailer-daemon",
    ];
    PATTERNS.iter().any(|p| local.contains(p))
}

fn extract_charset(content_type_lc: &str) -> Option<String> {
    let i = content_type_lc.find("charset")?;
    let rest = &content_type_lc[i + "charset".len()..];
    let rest = rest.trim_start_matches([' ', '=', '"', '\'']);
    let cs: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if cs.is_empty() { None } else { Some(cs) }
}

/// Extract lowercased domains from `http(s)://` URLs in the body.
fn url_domains(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lc = body.to_lowercase();
    let mut rest = lc.as_str();
    while let Some(i) = rest.find("http") {
        rest = &rest[i..];
        let after = rest
            .strip_prefix("https://")
            .or_else(|| rest.strip_prefix("http://"));
        if let Some(url) = after {
            let host: String = url
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                .collect();
            if host.contains('.') {
                out.push(host);
            }
            rest = &rest[4..];
        } else {
            rest = &rest[4..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_headers_and_body() {
        let msg = b"Subject: Hello World\r\nFrom: a@evil.example\r\n\r\nBuy cheap pills now";
        let toks = tokenize(msg);
        assert!(toks.contains(&"sub:hello".to_string()));
        assert!(toks.contains(&"sub:world".to_string()));
        assert!(toks.contains(&"from:evil.example".to_string()));
        assert!(toks.contains(&"cheap".to_string()));
        assert!(toks.contains(&"pills".to_string()));
        // "now" is 3 chars — kept; "a" too short — dropped.
        assert!(toks.contains(&"now".to_string()));
    }

    #[test]
    fn dedups_repeated_words() {
        let msg = b"Subject: spam\r\n\r\nspam spam spam eggs spam";
        let toks = tokenize(msg);
        let body_spam = toks.iter().filter(|t| t.as_str() == "spam").count();
        assert_eq!(body_spam, 1, "body word deduped");
    }

    #[test]
    fn strips_html_tags() {
        let msg = b"Subject: x\r\n\r\n<html><body><a href=\"http://bad.example/x\">click</a></body></html>";
        let toks = tokenize(msg);
        assert!(toks.contains(&"click".to_string()));
        assert!(toks.contains(&"url:bad.example".to_string()));
        assert!(!toks.iter().any(|t| t.contains("html")));
    }

    #[test]
    fn extracts_content_type_features() {
        let msg = b"Content-Type: text/html; charset=utf-8\r\n\r\nhi";
        let toks = tokenize(msg);
        assert!(toks.contains(&"hdr:ct:text/html".to_string()));
        assert!(toks.contains(&"hdr:charset:utf-8".to_string()));
    }

    #[test]
    fn cjk_bigram_split() {
        // Four Han chars → three bigrams.
        let msg = "Subject: x\r\n\r\n发票代开优惠".as_bytes();
        let toks = tokenize(msg);
        assert!(
            toks.iter()
                .any(|t| t.chars().count() == 2 && t.chars().all(is_cjk))
        );
    }

    #[test]
    fn caps_token_count() {
        let big = format!(
            "Subject: x\r\n\r\n{}",
            (0..2000).map(|i| format!("word{i} ")).collect::<String>()
        );
        let toks = tokenize(big.as_bytes());
        assert!(toks.len() <= MAX_TOKENS);
    }

    #[test]
    fn emits_triage_header_signal_tokens() {
        let raw = b"From: GitHub <noreply@github.com>\r\n\
                    List-Unsubscribe: <https://x/u>\r\n\
                    List-Id: repo.github.com\r\n\
                    Precedence: bulk\r\n\
                    Auto-Submitted: auto-generated\r\n\
                    Subject: notice\r\n\r\nbody";
        let toks = tokenize(raw);
        for t in [
            "from:automated",
            "hdr:list-unsub",
            "hdr:list-id",
            "hdr:precedence:bulk",
            "hdr:auto-submitted",
        ] {
            assert!(toks.contains(&t.to_string()), "missing token {t}");
        }
    }

    #[test]
    fn auto_submitted_no_is_not_a_signal() {
        let raw = b"From: a@b.com\r\nAuto-Submitted: no\r\nSubject: hi\r\n\r\nx";
        let toks = tokenize(raw);
        assert!(!toks.contains(&"hdr:auto-submitted".to_string()));
        assert!(!toks.contains(&"from:automated".to_string()));
    }
}
