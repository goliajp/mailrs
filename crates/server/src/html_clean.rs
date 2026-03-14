// deep html cleaning: multi-stage pipeline to extract readable content from email html

/// known tracking pixel domains
const TRACKING_DOMAINS: &[&str] = &[
    "mailchimp.com", "sendgrid.net", "hubspot.com", "mailgun.org",
    "constantcontact.com", "campaign-archive.com", "list-manage.com",
    "exacttarget.com", "sailthru.com", "marketo.com", "pardot.com",
    "braze.com", "iterable.com", "customer.io", "intercom-mail.com",
    "mandrillapp.com", "amazonses.com", "postmarkapp.com",
];

/// tracking pixel url path keywords
const TRACKING_PATHS: &[&str] = &[
    "/track", "/pixel", "/beacon", "/open", "/wf/open", "/o/", "/t/",
    "/imp", "/ci/", "/e/o/", "tracking", "1x1",
];

/// footer keywords (multi-language)
const FOOTER_KEYWORDS: &[&str] = &[
    "unsubscribe", "opt-out", "opt out", "manage preferences",
    "email preferences", "update preferences", "subscription",
    "配信停止", "退订", "取消订阅", "メール配信", "購読解除",
    "view in browser", "view this email", "ブラウザで表示",
    "privacy policy", "terms of service", "all rights reserved",
    "©", "you are receiving this",
    "this email was sent to", "no longer wish to receive",
    "if you no longer", "to stop receiving",
];

/// result of html cleaning
pub(crate) struct CleanResult {
    pub clean_text: String,
    pub has_tracking_pixel: bool,
    pub is_template_heavy: bool,
    pub link_count: usize,
    #[allow(dead_code)]
    pub image_count: usize,
    #[allow(dead_code)]
    pub text_to_html_ratio: f32,
}

/// clean html email content through multi-stage pipeline
pub(crate) fn clean_email_html(html: &str) -> CleanResult {
    let has_tracking_pixel = detect_tracking_pixels(html);
    let link_count = count_pattern(html, "<a ");
    let image_count = count_pattern(html, "<img ");

    // stage 1: remove script, style, comments
    let stage1 = remove_unsafe_elements(html);

    // stage 2: remove tracking pixels and hidden elements
    let stage2 = remove_tracking_and_hidden(&stage1);

    // stage 3: remove template header/footer
    let (stage3, footer_removed) = remove_template_chrome(&stage2);

    // stage 4: convert to clean text
    let clean_text = html_to_clean_text(&stage3);

    let text_to_html_ratio = if html.is_empty() {
        1.0
    } else {
        clean_text.len() as f32 / html.len() as f32
    };

    // template-heavy: low text ratio + many images or footer removed
    let is_template_heavy = text_to_html_ratio < 0.05 || (footer_removed && text_to_html_ratio < 0.15);

    CleanResult {
        clean_text,
        has_tracking_pixel,
        is_template_heavy,
        link_count,
        image_count,
        text_to_html_ratio,
    }
}

/// detect tracking pixels in raw html
fn detect_tracking_pixels(html: &str) -> bool {
    let lower = html.to_lowercase();

    // check for 1x1 or 0x0 images
    for img_start in find_all_positions(&lower, "<img") {
        let img_end = match lower[img_start..].find('>') {
            Some(p) => img_start + p + 1,
            None => continue,
        };
        let tag = &lower[img_start..img_end];

        // size-based detection
        let is_tiny = (tag.contains("width=\"1\"") || tag.contains("width='1'")
            || tag.contains("width:1") || tag.contains("width: 1")
            || tag.contains("width=\"0\"") || tag.contains("width='0'"))
            && (tag.contains("height=\"1\"") || tag.contains("height='1'")
                || tag.contains("height:1") || tag.contains("height: 1")
                || tag.contains("height=\"0\"") || tag.contains("height='0'"));

        // hidden via css
        let is_hidden = tag.contains("display:none") || tag.contains("display: none")
            || tag.contains("visibility:hidden") || tag.contains("visibility: hidden")
            || tag.contains("opacity:0") || tag.contains("opacity: 0");

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
fn remove_unsafe_elements(html: &str) -> String {
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
fn remove_tracking_and_hidden(html: &str) -> String {
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
            let is_tiny = (tag_lower.contains("width=\"1\"") || tag_lower.contains("width='1'")
                || tag_lower.contains("width=\"0\"") || tag_lower.contains("width='0'"))
                && (tag_lower.contains("height=\"1\"") || tag_lower.contains("height='1'")
                    || tag_lower.contains("height=\"0\"") || tag_lower.contains("height='0'"));

            let is_hidden = tag_lower.contains("display:none") || tag_lower.contains("display: none")
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
fn remove_hidden_blocks(html: &str) -> String {
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
                if let Some(tag_name) = extract_tag_name(tag_lower) {
                    if let Some(end) = find_closing_tag(&lower[tag_start..], &tag_name) {
                        result.push_str(&html[pos..tag_start]);
                        pos = tag_start + end;
                        continue;
                    }
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
fn remove_template_chrome(html: &str) -> (String, bool) {
    let lower = html.to_lowercase();
    let mut footer_removed = false;

    // find the last occurrence of footer keywords
    let mut earliest_footer = html.len();
    for keyword in FOOTER_KEYWORDS {
        if let Some(pos) = lower.rfind(keyword) {
            // find the enclosing block element (td, div, tr)
            if let Some(block_start) = find_enclosing_block_start(&lower[..pos]) {
                if block_start < earliest_footer {
                    earliest_footer = block_start;
                    footer_removed = true;
                }
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
fn html_to_clean_text(html: &str) -> String {
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

/// detect if sender is a bulk/automated sender based on email headers
pub(crate) fn detect_bulk_sender(raw_headers: &str) -> bool {
    let lower = raw_headers.to_lowercase();

    // list-unsubscribe header
    if lower.contains("list-unsubscribe:") {
        return true;
    }

    // precedence: bulk or list
    if lower.contains("precedence: bulk") || lower.contains("precedence: list")
        || lower.contains("precedence:bulk") || lower.contains("precedence:list")
    {
        return true;
    }

    // x-mailer headers from known ESPs
    if lower.contains("x-sg-id") || lower.contains("x-mailgun-") || lower.contains("x-mandrill-")
        || lower.contains("x-mc-") || lower.contains("x-ses-")
        || lower.contains("x-campaign") || lower.contains("x-mailer: mailchimp")
    {
        return true;
    }

    // auto-submitted header
    if lower.contains("auto-submitted:") {
        let auto_val = lower.split("auto-submitted:").nth(1).unwrap_or("");
        let auto_val = auto_val.split('\n').next().unwrap_or("").trim();
        if auto_val != "no" {
            return true;
        }
    }

    false
}

/// detect automated/noreply senders
pub(crate) fn is_automated_sender(email: &str) -> bool {
    let lower = email.to_lowercase();
    let local = lower.split('@').next().unwrap_or("");

    local == "noreply" || local == "no-reply" || local == "do-not-reply"
        || local == "donotreply" || local == "mailer-daemon"
        || local == "postmaster" || local.starts_with("bounce")
        || local.starts_with("notification") || local == "auto"
}

/// extract quoted text boundary from email text
/// returns (new_content, quoted_parts) where new_content is the original reply text
pub(crate) fn split_quoted_content(text: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut split_point = lines.len();
    let mut quoted = Vec::new();

    // find "On ... wrote:" pattern (supports multiple languages)
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // english: "On Mon, Jan 1, 2025 at 10:00 AM Alice <alice@x.com> wrote:"
        if (trimmed.starts_with("On ") && trimmed.ends_with("wrote:"))
            || (trimmed.starts_with("On ") && trimmed.contains(" wrote:"))
        {
            split_point = i;
            break;
        }

        // japanese: "2025年1月1日 10:00 Alice <alice@x.com>:"
        if trimmed.contains("年") && trimmed.contains("月") && trimmed.contains("日")
            && trimmed.ends_with(':')
            && trimmed.contains('@')
        {
            split_point = i;
            break;
        }

        // outlook style: "From: Alice" followed by "Sent:" or "Date:"
        if trimmed.starts_with("From:") && i + 1 < lines.len() {
            let next = lines[i + 1].trim();
            if next.starts_with("Sent:") || next.starts_with("Date:") || next.starts_with("日時:") {
                split_point = i;
                break;
            }
        }

        // simple quote prefix: line starting with ">"
        // only if it's a block (3+ consecutive lines)
        if trimmed.starts_with('>') {
            let mut count = 1;
            for line in lines.iter().skip(i + 1) {
                if line.trim().starts_with('>') {
                    count += 1;
                } else {
                    break;
                }
            }
            if count >= 3 {
                split_point = i;
                break;
            }
        }

        // separator line: "----" or "____" or "====" (at least 4 chars)
        if (trimmed.starts_with("----") || trimmed.starts_with("____") || trimmed.starts_with("===="))
            && trimmed.len() >= 4
            && i > 0
        {
            // check if next line looks like quoted header
            if i + 1 < lines.len() {
                let next = lines[i + 1].trim();
                if next.starts_with("From:") || next.starts_with("Subject:") || next.starts_with("Date:") {
                    split_point = i;
                    break;
                }
            }
        }
    }

    if split_point < lines.len() {
        let new_content = lines[..split_point].join("\n").trim_end().to_string();
        let quoted_text = lines[split_point..].join("\n");
        quoted.push(quoted_text);
        (new_content, quoted)
    } else {
        (text.to_string(), quoted)
    }
}

// ---- helper functions ----

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

fn count_pattern(html: &str, pattern: &str) -> usize {
    let lower = html.to_lowercase();
    lower.matches(pattern).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_simple_html() {
        let html = "<html><body><p>Hello world</p></body></html>";
        let result = clean_email_html(html);
        assert!(result.clean_text.contains("Hello world"));
        assert!(!result.has_tracking_pixel);
    }

    #[test]
    fn detect_1x1_tracking_pixel() {
        let html = r#"<img src="https://track.mailchimp.com/open?u=123" width="1" height="1">"#;
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn detect_hidden_pixel() {
        let html = r#"<img src="https://example.com/pixel" style="display:none">"#;
        assert!(detect_tracking_pixels(html));
    }

    #[test]
    fn no_tracking_pixel_normal_image() {
        let html = r#"<img src="https://example.com/photo.jpg" width="600" height="400">"#;
        assert!(!detect_tracking_pixels(html));
    }

    #[test]
    fn remove_script_tags() {
        let html = "<p>Before</p><script>alert('xss')</script><p>After</p>";
        let cleaned = remove_unsafe_elements(html);
        assert!(!cleaned.contains("alert"));
        assert!(cleaned.contains("Before"));
        assert!(cleaned.contains("After"));
    }

    #[test]
    fn remove_style_tags() {
        let html = "<style>.foo{color:red}</style><p>Content</p>";
        let cleaned = remove_unsafe_elements(html);
        assert!(!cleaned.contains("color:red"));
        assert!(cleaned.contains("Content"));
    }

    #[test]
    fn remove_html_comments() {
        let html = "<p>A</p><!--[if mso]><table><tr><td>Outlook junk</td></tr></table><![endif]--><p>B</p>";
        let cleaned = remove_unsafe_elements(html);
        assert!(!cleaned.contains("Outlook junk"));
        assert!(cleaned.contains("A"));
        assert!(cleaned.contains("B"));
    }

    #[test]
    fn detect_bulk_sender_list_unsubscribe() {
        let headers = "From: news@example.com\r\nList-Unsubscribe: <mailto:unsub@example.com>\r\n";
        assert!(detect_bulk_sender(headers));
    }

    #[test]
    fn detect_bulk_sender_precedence() {
        let headers = "From: news@example.com\r\nPrecedence: bulk\r\n";
        assert!(detect_bulk_sender(headers));
    }

    #[test]
    fn not_bulk_sender_personal() {
        let headers = "From: alice@example.com\r\nTo: bob@example.com\r\n";
        assert!(!detect_bulk_sender(headers));
    }

    #[test]
    fn is_automated_sender_noreply() {
        assert!(is_automated_sender("noreply@example.com"));
        assert!(is_automated_sender("no-reply@example.com"));
        assert!(is_automated_sender("NOREPLY@Example.com"));
    }

    #[test]
    fn is_not_automated_sender() {
        assert!(!is_automated_sender("alice@example.com"));
        assert!(!is_automated_sender("support@example.com"));
    }

    #[test]
    fn split_quoted_on_wrote() {
        let text = "Thanks for the update.\n\nOn Mon, Jan 1, 2025 at 10:00 AM Alice <alice@x.com> wrote:\n> Original message";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "Thanks for the update.");
        assert_eq!(quoted.len(), 1);
        assert!(quoted[0].contains("Alice"));
    }

    #[test]
    fn split_quoted_no_quote() {
        let text = "Just a simple message with no quotes.";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, text);
        assert!(quoted.is_empty());
    }

    #[test]
    fn split_quoted_outlook_style() {
        let text = "My reply.\n\nFrom: Bob\nSent: Monday\nTo: Alice\nSubject: Re: test\n\nOriginal";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "My reply.");
        assert_eq!(quoted.len(), 1);
    }

    #[test]
    fn split_quoted_angle_bracket() {
        let text = "My reply.\n\n> line 1\n> line 2\n> line 3\n> line 4";
        let (new_content, quoted) = split_quoted_content(text);
        assert_eq!(new_content, "My reply.");
        assert_eq!(quoted.len(), 1);
    }

    #[test]
    fn text_to_html_ratio_pure_text() {
        let html = "Hello world";
        let result = clean_email_html(html);
        assert!(result.text_to_html_ratio > 0.5);
    }

    #[test]
    fn text_to_html_ratio_heavy_template() {
        let html = "<html><head><style>.a{}.b{}.c{}</style></head><body><table><tr><td><table><tr><td><img src='logo.png' width='600'></td></tr></table></td></tr><tr><td>Hi</td></tr><tr><td><a href='#'>Unsubscribe</a></td></tr></table></body></html>";
        let result = clean_email_html(html);
        // template-heavy emails have low ratio
        assert!(result.text_to_html_ratio < 0.3);
    }

    #[test]
    fn footer_removal() {
        let html = "<div>Important content here with lots of text that forms the main body of the email message.</div><div>More important content in the second paragraph.</div><div><a href='#'>unsubscribe</a> | <a href='#'>manage preferences</a></div>";
        let (result, removed) = remove_template_chrome(html);
        assert!(removed);
        assert!(result.contains("Important content"));
    }
}
