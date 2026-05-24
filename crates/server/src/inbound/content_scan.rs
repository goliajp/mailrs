// ClamAV INSTREAM client lives in mailrs-clamav 1.0.0; we re-export
// for back-compat with the previous internal API names.
pub use mailrs_clamav::{parse_response as parse_clamav_response, scan as scan_clamav, ClamavResult};

/// content scanning result
#[derive(Debug, Clone, PartialEq)]
pub enum ScanResult {
    Clean { score: f64 },
    Spam { score: f64, rules: Vec<String> },
    Virus { name: String },
}

/// content scoring rule
struct ScoringRule {
    name: &'static str,
    score: f64,
    check: fn(&[u8]) -> bool,
}

fn has_from_header(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    // check for From: header (case insensitive, at start of line)
    for line in s.lines() {
        if line.to_lowercase().starts_with("from:") {
            return true;
        }
    }
    false
}

fn has_subject_header(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    for line in s.lines() {
        if line.to_lowercase().starts_with("subject:") {
            let value = line[8..].trim();
            if !value.is_empty() {
                return true;
            }
        }
    }
    false
}

fn has_date_header(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    for line in s.lines() {
        if line.to_lowercase().starts_with("date:") {
            return true;
        }
    }
    false
}

fn has_message_id(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    for line in s.lines() {
        if line.to_lowercase().starts_with("message-id:") {
            return true;
        }
    }
    false
}

fn count_urls(data: &[u8]) -> usize {
    let s = String::from_utf8_lossy(data);
    s.matches("http://").count() + s.matches("https://").count()
}

fn has_suspicious_attachment(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    let suspicious = [
        ".exe", ".scr", ".bat", ".cmd", ".com", ".pif", ".vbs", ".js", ".wsf",
    ];
    for ext in &suspicious {
        if s.contains(ext) {
            // check if it's in a Content-Disposition or Content-Type
            for line in s.lines() {
                let lower = line.to_lowercase();
                if (lower.contains("content-disposition") || lower.contains("content-type"))
                    && lower.contains(ext)
                {
                    return true;
                }
            }
        }
    }
    false
}

fn is_html_only(data: &[u8]) -> bool {
    let s = String::from_utf8_lossy(data);
    let lower = s.to_lowercase();
    lower.contains("content-type: text/html") && !lower.contains("content-type: text/plain")
}

/// evaluate all content rules and return total score + matched rule names
pub fn evaluate_rules(data: &[u8]) -> (f64, Vec<String>) {
    let rules: Vec<ScoringRule> = vec![
        ScoringRule {
            name: "missing_from",
            score: 2.0,
            check: |d| !has_from_header(d),
        },
        ScoringRule {
            name: "empty_subject",
            score: 1.0,
            check: |d| !has_subject_header(d),
        },
        ScoringRule {
            name: "html_only_no_text",
            score: 1.5,
            check: is_html_only,
        },
        ScoringRule {
            name: "excessive_urls",
            score: 2.0,
            check: |d| count_urls(d) > 10,
        },
        ScoringRule {
            name: "suspicious_attachment",
            score: 3.0,
            check: has_suspicious_attachment,
        },
        ScoringRule {
            name: "missing_date",
            score: 1.0,
            check: |d| !has_date_header(d),
        },
        ScoringRule {
            name: "missing_message_id",
            score: 1.5,
            check: |d| !has_message_id(d),
        },
    ];

    let mut total = 0.0;
    let mut matched = Vec::new();

    for rule in &rules {
        if (rule.check)(data) {
            total += rule.score;
            matched.push(rule.name.to_string());
        }
    }

    (total, matched)
}

#[cfg(test)]
#[path = "content_scan_tests.rs"]
mod tests;
