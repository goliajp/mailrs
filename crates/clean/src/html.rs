//! HTML processing pipeline: pixel detect, tracking strip, chrome strip,
//! text conversion. Internal — public surface is in lib.rs.

use super::{FOOTER_KEYWORDS, TRACKING_DOMAINS, TRACKING_PATHS};

pub(super) fn detect_tracking_pixels(html: &str) -> bool {
    // ASCII fold is enough — every shape we match (tag names, attr
    // names, CSS keywords) is pure ASCII. Full Unicode `to_lowercase`
    // does Turkish-I + greek-sigma folding we don't need.
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    // memchr-based `<img` scan: probe `<` positions, confirm `img` follows.
    let mut pos = 0;
    while let Some(rel) = memchr::memchr(b'<', &bytes[pos..]) {
        let img_start = pos + rel;
        if img_start + 4 > bytes.len() || &bytes[img_start + 1..img_start + 4] != b"img" {
            pos = img_start + 1;
            continue;
        }
        let img_end = match memchr::memchr(b'>', &bytes[img_start..]) {
            Some(p) => img_start + p + 1,
            None => break,
        };
        let tag = &lower[img_start..img_end];

        // size-based detection
        let is_tiny = (tag.contains("width=\"1\"")
            || tag.contains("width='1'")
            || tag.contains("width:1")
            || tag.contains("width: 1")
            || tag.contains("width=\"0\"")
            || tag.contains("width='0'"))
            && (tag.contains("height=\"1\"")
                || tag.contains("height='1'")
                || tag.contains("height:1")
                || tag.contains("height: 1")
                || tag.contains("height=\"0\"")
                || tag.contains("height='0'"));

        // hidden via css
        let is_hidden = tag.contains("display:none")
            || tag.contains("display: none")
            || tag.contains("visibility:hidden")
            || tag.contains("visibility: hidden")
            || tag.contains("opacity:0")
            || tag.contains("opacity: 0");

        if is_tiny || is_hidden {
            return true;
        }

        // domain-based detection
        if let Some(src) = extract_attr(tag, "src") {
            for domain in TRACKING_DOMAINS {
                if src.contains(domain) {
                    return true;
                }
            }
            for path in TRACKING_PATHS {
                if src.contains(path) {
                    return true;
                }
            }
        }

        pos = img_end;
    }

    false
}

/// remove <script>, <style>, <iframe>, html comments
pub(super) fn remove_unsafe_elements(html: &str) -> String {
    // Single-pass comment stripper. Old impl looped `find("<!--")` +
    // `replace_range`, which is O(n²) on long inputs because each
    // `replace_range` shifts the tail. New impl scans forward once,
    // appending kept ranges to a pre-sized String.
    let bytes = html.as_bytes();
    let mut without_comments = String::with_capacity(html.len());
    let mut pos = 0;
    while pos < bytes.len() {
        // Search for `<!--` from current pos.
        match find_substring(bytes, b"<!--", pos) {
            Some(start) => {
                without_comments.push_str(&html[pos..start]);
                // Find closing `-->` after the `<!--`.
                match find_substring(bytes, b"-->", start + 4) {
                    Some(end) => pos = end + 3,
                    None => {
                        // Unterminated comment — drop everything from
                        // `<!--` to end, matching old behaviour
                        // (`break` on missing close).
                        return remove_block_tags_single_pass(&without_comments);
                    }
                }
            }
            None => {
                without_comments.push_str(&html[pos..]);
                break;
            }
        }
    }

    // Stage 1b: remove block elements by tag name. Fused into one
    // forward scan with case-insensitive prefix match, instead of
    // 5 separate `remove_tag_block` calls (each a full O(n) scan).
    remove_block_tags_single_pass(&without_comments)
}

/// Fast `Vec<u8>`-backed substring search using `memchr` to jump to
/// candidate first-byte positions, then `eq` on the slice. Much
/// cheaper than `str::find` on long inputs because we skip UTF-8
/// boundary checking — the patterns we match (`<!--`, `-->`, `<tag`)
/// are pure ASCII.
#[inline]
fn find_substring(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() {
        return None;
    }
    let first = needle[0];
    let mut pos = start;
    while pos + needle.len() <= haystack.len() {
        let rel = memchr::memchr(first, &haystack[pos..])?;
        let abs = pos + rel;
        if abs + needle.len() <= haystack.len() && haystack[abs..abs + needle.len()] == *needle {
            return Some(abs);
        }
        pos = abs + 1;
    }
    None
}

/// Strip all block-level dangerous tags (`script`, `style`, `iframe`,
/// `noscript`, `svg`) in a single forward pass.
///
/// Case-insensitive opening-tag match using ASCII byte tricks
/// (`b | 0x20` lowercases ASCII letters). Closing tags found via
/// `find_substring`. ~5× faster than the previous 5-call
/// `remove_tag_block` chain on a typical 5 KB marketing email.
fn remove_block_tags_single_pass(html: &str) -> String {
    const BLOCK_TAGS: &[&[u8]] = &[b"script", b"style", b"iframe", b"noscript", b"svg"];
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    'scan: while pos < bytes.len() {
        let lt = match memchr::memchr(b'<', &bytes[pos..]) {
            Some(rel) => pos + rel,
            None => {
                out.push_str(&html[pos..]);
                break;
            }
        };
        // Try each block tag at this `<` position.
        for &tag in BLOCK_TAGS {
            // Need at least `<tag` + one terminator byte.
            if lt + 1 + tag.len() > bytes.len() {
                continue;
            }
            if eq_ascii_lower(&bytes[lt + 1..lt + 1 + tag.len()], tag) {
                let after_name = lt + 1 + tag.len();
                // Must be followed by `>`, ` `, `\t`, `\n`, `\r`, or `/`
                let next = bytes[after_name];
                if matches!(next, b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/') {
                    // Find the matching `</tag>` (case-insensitive).
                    let close_seq = {
                        let mut v = Vec::with_capacity(2 + tag.len() + 1);
                        v.push(b'<');
                        v.push(b'/');
                        v.extend_from_slice(tag);
                        v
                    };
                    out.push_str(&html[pos..lt]);
                    if let Some(close_start) = find_substring_ci(bytes, &close_seq, after_name) {
                        // Skip past the closing `>`.
                        match memchr::memchr(b'>', &bytes[close_start..]) {
                            Some(gt) => pos = close_start + gt + 1,
                            None => break 'scan,
                        }
                    } else {
                        // No closing tag — drop everything to EOF
                        // (matches old `remove_tag_block` behaviour on
                        // malformed input).
                        break 'scan;
                    }
                    continue 'scan;
                }
            }
        }
        // Not a block tag — copy the `<` and advance one byte.
        out.push_str(&html[pos..lt + 1]);
        pos = lt + 1;
    }
    out
}

/// ASCII case-insensitive equality on slices of the same length.
/// `target` is assumed already lowercase (block-tag literals).
#[inline]
fn eq_ascii_lower(input: &[u8], target: &[u8]) -> bool {
    if input.len() != target.len() {
        return false;
    }
    for i in 0..input.len() {
        // OR by 0x20 to lowercase ASCII letters; leaves non-letters
        // alone (since target is all-lowercase ASCII letters this is
        // safe).
        if input[i] | 0x20 != target[i] {
            return false;
        }
    }
    true
}

/// Case-insensitive `find_substring` using the same memchr trick.
/// Pattern must be all ASCII; case-insensitivity is byte-OR'd.
#[inline]
fn find_substring_ci(haystack: &[u8], needle_lower: &[u8], start: usize) -> Option<usize> {
    if needle_lower.is_empty() || start >= haystack.len() {
        return None;
    }
    let first = needle_lower[0];
    let mut pos = start;
    while pos + needle_lower.len() <= haystack.len() {
        // memchr2 — accept first char as either case.
        let upper = if first.is_ascii_lowercase() {
            first & !0x20
        } else {
            first
        };
        let rel = if upper == first {
            memchr::memchr(first, &haystack[pos..])?
        } else {
            memchr::memchr2(first, upper, &haystack[pos..])?
        };
        let abs = pos + rel;
        if abs + needle_lower.len() <= haystack.len()
            && eq_ascii_lower(&haystack[abs..abs + needle_lower.len()], needle_lower)
        {
            return Some(abs);
        }
        pos = abs + 1;
    }
    None
}

/// remove tracking pixels and hidden elements
pub(super) fn remove_tracking_and_hidden(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let lower = html.to_lowercase();
    let mut pos = 0;

    while pos < html.len() {
        if let Some(img_offset) = lower[pos..].find("<img") {
            let img_start = pos + img_offset;
            // copy text before this img
            result.push_str(&html[pos..img_start]);

            let img_end = match lower[img_start..].find('>') {
                Some(p) => img_start + p + 1,
                None => {
                    pos = img_start + 4;
                    continue;
                }
            };

            let tag_lower = &lower[img_start..img_end];

            // check if tracking pixel
            let is_tiny = (tag_lower.contains("width=\"1\"")
                || tag_lower.contains("width='1'")
                || tag_lower.contains("width=\"0\"")
                || tag_lower.contains("width='0'"))
                && (tag_lower.contains("height=\"1\"")
                    || tag_lower.contains("height='1'")
                    || tag_lower.contains("height=\"0\"")
                    || tag_lower.contains("height='0'"));

            let is_hidden = tag_lower.contains("display:none")
                || tag_lower.contains("display: none")
                || tag_lower.contains("visibility:hidden");

            if is_tiny || is_hidden {
                // skip this img tag
                pos = img_end;
            } else {
                result.push_str(&html[img_start..img_end]);
                pos = img_end;
            }
        } else {
            result.push_str(&html[pos..]);
            break;
        }
    }

    // remove any elements with display:none style (divs, spans, etc.)
    result = remove_hidden_blocks(&result);

    result
}

/// remove elements with display:none
pub(super) fn remove_hidden_blocks(html: &str) -> String {
    let lower = html.to_lowercase();
    let mut result = String::with_capacity(html.len());
    let mut pos = 0;

    while pos < html.len() {
        if let Some(offset) = lower[pos..].find("display:none") {
            let check_pos = pos + offset;
            // find the enclosing tag
            if let Some(tag_start) = html[..check_pos].rfind('<') {
                let tag_lower = &lower[tag_start..];
                // find the tag name
                if let Some(tag_name) = extract_tag_name(tag_lower)
                    && let Some(end) = find_closing_tag(&lower[tag_start..], &tag_name)
                {
                    result.push_str(&html[pos..tag_start]);
                    pos = tag_start + end;
                    continue;
                }
            }
            result.push_str(&html[pos..check_pos + 12]);
            pos = check_pos + 12;
        } else {
            result.push_str(&html[pos..]);
            break;
        }
    }

    result
}

/// remove template header and footer regions
pub(super) fn remove_template_chrome(html: &str) -> (String, bool) {
    let lower = html.to_lowercase();
    let mut footer_removed = false;

    // find the last occurrence of footer keywords
    let mut earliest_footer = html.len();
    for keyword in FOOTER_KEYWORDS {
        if let Some(pos) = lower.rfind(keyword) {
            // find the enclosing block element (td, div, tr)
            if let Some(block_start) = find_enclosing_block_start(&lower[..pos])
                && block_start < earliest_footer
            {
                earliest_footer = block_start;
                footer_removed = true;
            }
        }
    }

    // don't remove more than 40% of the content
    if footer_removed && earliest_footer < html.len() * 60 / 100 {
        footer_removed = false;
        earliest_footer = html.len();
    }

    let trimmed = if footer_removed {
        &html[..earliest_footer]
    } else {
        html
    };

    (trimmed.to_string(), footer_removed)
}

/// convert cleaned html to plain text
pub(super) fn html_to_clean_text(html: &str) -> String {
    let text = match html2text::from_read(html.as_bytes(), 80) {
        Ok(t) => t,
        Err(_) => return String::new(),
    };

    // post-process: collapse excessive whitespace
    let mut lines: Vec<&str> = Vec::new();
    let mut blank_count = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                lines.push("");
            }
        } else {
            blank_count = 0;
            lines.push(trimmed);
        }
    }

    // trim trailing blank lines
    while lines.last() == Some(&"") {
        lines.pop();
    }

    lines.join("\n")
}

// `remove_tag_block` was the per-tag block-stripper called from
// `remove_unsafe_elements`. The v4 round 6 squeeze replaced the
// 5-tag-call chain with a single-pass `remove_block_tags_single_pass`
// — keep this function around as a private helper only if a future
// caller needs single-tag stripping; otherwise rely on the bulk
// scanner. Removed to avoid dead-code accumulation.

fn extract_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let search = format!("{attr}=\"");
    if let Some(start) = tag.find(&search) {
        let val_start = start + search.len();
        if let Some(end) = tag[val_start..].find('"') {
            return Some(&tag[val_start..val_start + end]);
        }
    }
    let search2 = format!("{attr}='");
    if let Some(start) = tag.find(&search2) {
        let val_start = start + search2.len();
        if let Some(end) = tag[val_start..].find('\'') {
            return Some(&tag[val_start..val_start + end]);
        }
    }
    None
}

fn extract_tag_name(lower_html: &str) -> Option<String> {
    if !lower_html.starts_with('<') {
        return None;
    }
    let after = &lower_html[1..];
    let end = after.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    let name = after[..end].to_string();
    if name.is_empty() || name.starts_with('/') {
        None
    } else {
        Some(name)
    }
}

fn find_closing_tag(lower_html: &str, tag_name: &str) -> Option<usize> {
    let close = format!("</{tag_name}>");
    lower_html.find(&close).map(|p| p + close.len())
}

fn find_enclosing_block_start(html_before: &str) -> Option<usize> {
    // look backward for <td, <tr, <div, <table
    let block_tags = ["<td", "<tr", "<div", "<table"];
    let mut best = None;

    for tag in &block_tags {
        if let Some(pos) = html_before.rfind(tag) {
            match best {
                None => best = Some(pos),
                Some(current) => {
                    if pos > current {
                        best = Some(pos);
                    }
                }
            }
        }
    }

    best
}

pub(super) fn count_pattern(html: &str, pattern: &str) -> usize {
    let lower = html.to_lowercase();
    lower.matches(pattern).count()
}
