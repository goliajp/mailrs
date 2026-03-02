use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// scan message via ClamAV's INSTREAM protocol (clamd TCP socket)
pub async fn scan_clamav(addr: &str, data: &[u8]) -> ClamavResult {
    let mut stream = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(e) => return ClamavResult::Error(format!("connect failed: {e}")),
    };

    // send zINSTREAM command
    if stream.write_all(b"zINSTREAM\0").await.is_err() {
        return ClamavResult::Error("write command failed".into());
    }

    // send data in chunks (max 2MB each per ClamAV protocol)
    const CHUNK_SIZE: usize = 2 * 1024 * 1024;
    for chunk in data.chunks(CHUNK_SIZE) {
        let len = (chunk.len() as u32).to_be_bytes();
        if stream.write_all(&len).await.is_err() || stream.write_all(chunk).await.is_err() {
            return ClamavResult::Error("write data failed".into());
        }
    }

    // send terminator (zero-length chunk)
    if stream.write_all(&0u32.to_be_bytes()).await.is_err() {
        return ClamavResult::Error("write terminator failed".into());
    }

    // read response
    let mut response = vec![0u8; 1024];
    match stream.read(&mut response).await {
        Ok(n) => parse_clamav_response(&response[..n]),
        Err(e) => ClamavResult::Error(format!("read failed: {e}")),
    }
}

/// content scanning result
#[derive(Debug, Clone, PartialEq)]
pub enum ScanResult {
    Clean { score: f64 },
    Spam { score: f64, rules: Vec<String> },
    Virus { name: String },
}

/// ClamAV scan result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClamavResult {
    Clean,
    Virus(String),
    Error(String),
}

/// parse ClamAV INSTREAM response
pub fn parse_clamav_response(response: &[u8]) -> ClamavResult {
    let s = String::from_utf8_lossy(response);
    let s = s.trim_end_matches('\0').trim();

    if s.ends_with("OK") {
        ClamavResult::Clean
    } else if let Some(found_pos) = s.find("FOUND") {
        // format: "stream: VirusName FOUND"
        let virus = s[..found_pos]
            .trim()
            .rsplit(':')
            .next()
            .unwrap_or("")
            .trim();
        ClamavResult::Virus(virus.to_string())
    } else {
        ClamavResult::Error(s.to_string())
    }
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
    let suspicious = [".exe", ".scr", ".bat", ".cmd", ".com", ".pif", ".vbs", ".js", ".wsf"];
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
mod tests {
    use super::*;

    #[test]
    fn parse_clamav_clean() {
        assert_eq!(
            parse_clamav_response(b"stream: OK\0"),
            ClamavResult::Clean
        );
    }

    #[test]
    fn parse_clamav_virus() {
        assert_eq!(
            parse_clamav_response(b"stream: Eicar FOUND\0"),
            ClamavResult::Virus("Eicar".into())
        );
    }

    #[test]
    fn parse_clamav_error() {
        let result = parse_clamav_response(b"INSTREAM size limit exceeded\0");
        assert!(matches!(result, ClamavResult::Error(_)));
    }

    #[test]
    fn evaluate_missing_from() {
        let data = b"Subject: test\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <test@example.com>\r\n\r\nbody";
        let (score, rules) = evaluate_rules(data);
        assert!(score >= 2.0);
        assert!(rules.contains(&"missing_from".to_string()));
    }

    #[test]
    fn evaluate_clean_message() {
        let data = b"From: sender@example.com\r\nSubject: Hello\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: text/plain\r\n\r\nHello world";
        let (score, rules) = evaluate_rules(data);
        assert_eq!(score, 0.0);
        assert!(rules.is_empty());
    }

    #[test]
    fn evaluate_multiple_rules() {
        // missing From, missing Date, missing Message-ID, excessive URLs
        let mut data = String::from("Subject: test\r\n\r\n");
        for i in 0..15 {
            data.push_str(&format!("https://spam{i}.example.com "));
        }
        let (score, rules) = evaluate_rules(data.as_bytes());
        // missing_from(2) + missing_date(1) + missing_message_id(1.5) + excessive_urls(2)
        assert!(score >= 6.5);
        assert!(rules.contains(&"missing_from".to_string()));
        assert!(rules.contains(&"excessive_urls".to_string()));
    }

    #[test]
    fn evaluate_suspicious_attachment() {
        let data = b"From: sender@example.com\r\nSubject: Invoice\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: application/octet-stream; name=\"malware.exe\"\r\nContent-Disposition: attachment; filename=\"malware.exe\"\r\n\r\nbody";
        let (score, rules) = evaluate_rules(data);
        assert!(score >= 3.0);
        assert!(rules.contains(&"suspicious_attachment".to_string()));
    }

    #[test]
    fn count_urls_correctly() {
        let data = b"http://a.com https://b.com http://c.com";
        assert_eq!(count_urls(data), 3);
    }

    #[test]
    fn url_boundary_10_no_trigger() {
        let mut data = String::from("From: a@b.com\r\nSubject: test\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@b.com>\r\n\r\n");
        for i in 0..10 {
            data.push_str(&format!("https://example{i}.com "));
        }
        let (_, rules) = evaluate_rules(data.as_bytes());
        assert!(!rules.contains(&"excessive_urls".to_string()));
    }

    #[test]
    fn url_boundary_11_triggers() {
        let mut data = String::from("From: a@b.com\r\nSubject: test\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@b.com>\r\n\r\n");
        for i in 0..11 {
            data.push_str(&format!("https://example{i}.com "));
        }
        let (_, rules) = evaluate_rules(data.as_bytes());
        assert!(rules.contains(&"excessive_urls".to_string()));
    }

    #[test]
    fn subject_only_spaces_triggers_empty_subject() {
        let data = b"From: a@b.com\r\nSubject:   \r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@b.com>\r\n\r\nbody";
        let (_, rules) = evaluate_rules(data);
        assert!(rules.contains(&"empty_subject".to_string()));
    }

    #[test]
    fn multipart_alternative_no_html_only() {
        let data = b"From: a@b.com\r\nSubject: test\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@b.com>\r\nContent-Type: text/html\r\nContent-Type: text/plain\r\n\r\nbody";
        let (_, rules) = evaluate_rules(data);
        assert!(!rules.contains(&"html_only_no_text".to_string()));
    }

    #[test]
    fn from_not_on_first_line_still_detected() {
        let data = b"Subject: test\r\nFrom: sender@example.com\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@b.com>\r\n\r\nbody";
        let (_, rules) = evaluate_rules(data);
        assert!(!rules.contains(&"missing_from".to_string()));
    }

    #[test]
    fn js_in_body_not_in_header_no_trigger() {
        let data = b"From: a@b.com\r\nSubject: test\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\nMessage-ID: <1@b.com>\r\nContent-Type: text/plain\r\n\r\ncheck out file.js for details";
        let (_, rules) = evaluate_rules(data);
        assert!(!rules.contains(&"suspicious_attachment".to_string()));
    }

    #[test]
    fn all_rules_triggered_total_score() {
        // missing_from(2) + empty_subject(1) + html_only(1.5) + excessive_urls(2) + suspicious_attachment(3) + missing_date(1) + missing_message_id(1.5) = 12.0
        let mut data = String::from("Content-Type: text/html\r\nContent-Disposition: attachment; filename=\"malware.exe\"\r\n\r\n");
        for i in 0..15 {
            data.push_str(&format!("https://spam{i}.example.com "));
        }
        let (score, rules) = evaluate_rules(data.as_bytes());
        assert_eq!(score, 12.0);
        assert_eq!(rules.len(), 7);
    }

    #[test]
    fn clamav_response_multi_null_bytes() {
        let mut response = b"stream: OK".to_vec();
        response.extend_from_slice(&[0, 0, 0]);
        assert_eq!(parse_clamav_response(&response), ClamavResult::Clean);
    }
}
