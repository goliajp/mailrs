//! HTML processing pipeline: pixel detect, tracking strip, chrome strip,
//! text conversion. Internal — public surface is in lib.rs.

use super::{FOOTER_KEYWORDS, TRACKING_DOMAINS, TRACKING_PATHS};

pub(super) fn detect_tracking_pixels(html: &str) -> bool {
    let lower = html.to_lowercase();

    // check for 1x1 or 0x0 images
    for img_start in find_all_positions(&lower, "<img") {
        let img_end = match lower[img_start..].find('>') {
            Some(p) => img_start + p + 1,
            None => continue,
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
    }

    false
}

/// remove <script>, <style>, <iframe>, html comments
pub(super) fn remove_unsafe_elements(html: &str) -> String {
    let mut result = html.to_string();

    // remove html comments <!-- ... -->
    while let Some(start) = result.find("<!--") {
        if let Some(end) = result[start..].find("-->") {
            result.replace_range(start..start + end + 3, "");
        } else {
            break;
        }
    }

    // remove block elements by tag name
    for tag in &["script", "style", "iframe", "noscript", "svg"] {
        result = remove_tag_block(&result, tag);
    }

    result
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

fn remove_tag_block(html: &str, tag: &str) -> String {
    let lower = html.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut result = String::with_capacity(html.len());
    let mut pos = 0;

    while pos < html.len() {
        if let Some(offset) = lower[pos..].find(&open) {
            let start = pos + offset;
            result.push_str(&html[pos..start]);

            if let Some(end_offset) = lower[start..].find(&close) {
                pos = start + end_offset + close.len();
            } else {
                // no closing tag, skip to end of opening tag
                if let Some(gt) = html[start..].find('>') {
                    pos = start + gt + 1;
                } else {
                    pos = html.len();
                }
            }
        } else {
            result.push_str(&html[pos..]);
            break;
        }
    }

    result
}

fn find_all_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        positions.push(start + pos);
        start = start + pos + needle.len();
    }
    positions
}

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
