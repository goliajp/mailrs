//! Quoted-reply splitter — separate the author's new content from the
//! quoted history above/below.

/// extract quoted text boundary from email text
/// returns (new_content, quoted_parts) where new_content is the original reply text
pub fn split_quoted_content(text: &str) -> (String, Vec<String>) {
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

