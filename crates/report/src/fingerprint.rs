//! The body's SimHash: what stays the same when a template is sent to many
//! people with the names, numbers and tracking links changed.
//!
//! The text is the `text/plain` part, else the `text/html` part with its
//! tags, styles and scripts dropped. Before hashing it is lowercased, the
//! recipient's own name and address are removed, every digit is removed,
//! and a link keeps only its host. Words are the tokens; a run of CJK
//! characters is cut into overlapping pairs, since it has no spaces.

/// Fewer tokens than this and the body says too little to tell one template
/// from another: such a message has no fingerprint.
pub const MIN_TOKENS: usize = 8;

/// The SimHash of `raw`'s body, `None` when the body has fewer than
/// [`MIN_TOKENS`] tokens. `recipient` holds the strings that name the
/// reader — address, local part, display name — and are taken out first.
pub fn of_message(raw: &[u8], recipient: &[&str]) -> Option<u64> {
    let parsed = mailrs_mime::parse(raw);
    of_text(&body_of(&parsed), recipient)
}

/// The SimHash of already extracted text; see [`of_message`].
pub fn of_text(text: &str, recipient: &[&str]) -> Option<u64> {
    let tokens = tokens(&normalise(text, recipient));
    if tokens.len() < MIN_TOKENS {
        return None;
    }
    Some(simhash(&tokens))
}

/// The four 16-bit pieces a SimHash is indexed by. Two hashes at most three
/// bits apart share at least one piece, so looking up each piece finds
/// every candidate.
pub fn segments(h: u64) -> [u16; 4] {
    [
        h as u16,
        (h >> 16) as u16,
        (h >> 32) as u16,
        (h >> 48) as u16,
    ]
}

/// Bits in which `a` and `b` differ.
pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

fn body_of(part: &mailrs_mime::Part<'_>) -> String {
    let mut html = None;
    for p in part.walk() {
        match p.content_type.mime_type().as_str() {
            "text/plain" => {
                if let Some(t) = p.body_text() {
                    return t;
                }
            }
            "text/html" if html.is_none() => html = p.body_text(),
            _ => {}
        }
    }
    match html {
        Some(h) => strip_tags(&h),
        None => part.body_text().unwrap_or_default(),
    }
}

/// Tags removed, `<style>` and `<script>` with their content, each tag
/// replaced by a space so words on both sides stay apart.
fn strip_tags(html: &str) -> String {
    let lower = html.to_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let bytes = lower.as_bytes();
    while i < html.len() {
        if bytes[i] != b'<' {
            let ch = html[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let skip_to = ["style", "script"].iter().find_map(|t| {
            let open = format!("<{t}");
            lower[i..].starts_with(&open).then(|| {
                let close = format!("</{t}");
                lower[i..].find(&close).map(|e| i + e + close.len())
            })?
        });
        let from = skip_to.unwrap_or(i);
        i = match lower[from..].find('>') {
            Some(e) => from + e + 1,
            None => html.len(),
        };
        out.push(' ');
    }
    out
}

fn normalise(text: &str, recipient: &[&str]) -> String {
    let mut t = text.to_lowercase();
    let mut names: Vec<String> = recipient
        .iter()
        .map(|r| r.trim().to_lowercase())
        .filter(|r| r.chars().count() >= 2)
        .collect();
    // longest first, so an address goes before its local part
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in &names {
        t = t.replace(n.as_str(), " ");
    }
    hosts_only(&t)
        .chars()
        .map(|c| match c.is_numeric() {
            true => ' ',
            false => c,
        })
        .collect()
}

/// Every `http(s)://host/path?query` becomes `host`.
fn hosts_only(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(at) = rest.find("http") {
        let tail = &rest[at..];
        let scheme = ["https://", "http://"]
            .iter()
            .find(|s| tail.starts_with(**s))
            .map(|s| s.len());
        let Some(scheme) = scheme else {
            out.push_str(&rest[..at + 4]);
            rest = &rest[at + 4..];
            continue;
        };
        out.push_str(&rest[..at]);
        let link = &tail[scheme..];
        let end = link
            .find(|c: char| c.is_whitespace() || "\"'<>()".contains(c))
            .unwrap_or(link.len());
        let host_end = link[..end].find(['/', '?', '#']).unwrap_or(end);
        out.push(' ');
        out.push_str(&link[..host_end]);
        out.push(' ');
        rest = &link[end..];
    }
    out.push_str(rest);
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

fn tokens(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();
    let flush_word = |w: &mut String, out: &mut Vec<String>| {
        if w.chars().count() >= 2 {
            out.push(std::mem::take(w));
        }
        w.clear();
    };
    let flush_cjk = |run: &mut Vec<char>, out: &mut Vec<String>| {
        match run.len() {
            0 => {}
            1 => out.push(run[0].to_string()),
            _ => out.extend(run.windows(2).map(|p| p.iter().collect::<String>())),
        }
        run.clear();
    };
    for c in t.chars() {
        if is_cjk(c) {
            flush_word(&mut word, &mut out);
            cjk.push(c);
        } else if c.is_alphanumeric() || c == '.' && !word.is_empty() {
            flush_cjk(&mut cjk, &mut out);
            word.push(c);
        } else {
            flush_word(&mut word, &mut out);
            flush_cjk(&mut cjk, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_cjk(&mut cjk, &mut out);
    for w in &mut out {
        while w.ends_with('.') {
            w.pop();
        }
    }
    out.retain(|w| !w.is_empty());
    out
}

fn token_hash(t: &str) -> u64 {
    // FNV-1a, then SplitMix64 to spread FNV's weak low bits over all 64
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in t.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h = h.wrapping_add(0x9e37_79b9_7f4a_7c15);
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

fn simhash(tokens: &[String]) -> u64 {
    let mut v = [0i64; 64];
    for t in tokens {
        let h = token_hash(t);
        for (i, slot) in v.iter_mut().enumerate() {
            match (h >> i) & 1 {
                1 => *slot += 1,
                _ => *slot -= 1,
            }
        }
    }
    v.iter()
        .enumerate()
        .filter(|(_, s)| **s > 0)
        .fold(0u64, |acc, (i, _)| acc | (1u64 << i))
}

#[cfg(test)]
mod tests;
