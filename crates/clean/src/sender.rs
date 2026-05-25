//! Sender / header heuristics (bulk / automated detection).

/// detect if sender is a bulk/automated sender based on email headers
pub fn detect_bulk_sender(raw_headers: &str) -> bool {
    let lower = raw_headers.to_lowercase();

    // list-unsubscribe header
    if lower.contains("list-unsubscribe:") {
        return true;
    }

    // precedence: bulk or list
    if lower.contains("precedence: bulk")
        || lower.contains("precedence: list")
        || lower.contains("precedence:bulk")
        || lower.contains("precedence:list")
    {
        return true;
    }

    // x-mailer headers from known ESPs
    if lower.contains("x-sg-id")
        || lower.contains("x-mailgun-")
        || lower.contains("x-mandrill-")
        || lower.contains("x-mc-")
        || lower.contains("x-ses-")
        || lower.contains("x-campaign")
        || lower.contains("x-mailer: mailchimp")
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
pub fn is_automated_sender(email: &str) -> bool {
    let lower = email.to_lowercase();
    let local = lower.split('@').next().unwrap_or("");

    local == "noreply"
        || local == "no-reply"
        || local == "do-not-reply"
        || local == "donotreply"
        || local == "mailer-daemon"
        || local == "postmaster"
        || local.starts_with("bounce")
        || local.starts_with("notification")
        || local == "auto"
}
