//! What a message says on each kind of channel.

use serde_json::json;

use super::{Message, MessageKind, Sent};

impl Sent {
    fn text_line(&self, minimal: bool) -> String {
        match minimal {
            true => format!(
                "{} {} ({}) — {}",
                self.kind, self.rule, self.severity, self.object_kind
            ),
            false => format!(
                "{} {} ({}) — {} {}: {} [{}]",
                self.kind,
                self.rule,
                self.severity,
                self.object_kind,
                self.object,
                self.value,
                self.threshold
            ),
        }
    }
}

impl Message {
    /// The webhook body: every field, both languages.
    pub fn json(&self) -> serde_json::Value {
        let alerts: Vec<_> = self
            .notices
            .iter()
            .map(|n| {
                json!({
                    "rule": n.rule,
                    "state": n.kind,
                    "severity": n.severity,
                    "object_kind": n.object_kind,
                    "object": n.object,
                    "value": n.value,
                    "threshold": n.threshold,
                    "since": n.since,
                    "title": { "en": n.text.title_en, "zh": n.text.title_zh },
                    "meaning": { "en": n.text.meaning_en, "zh": n.text.meaning_zh },
                    "first_step": { "en": n.text.step_en, "zh": n.text.step_zh },
                    "filed_under": n.under,
                })
            })
            .collect();
        json!({
            "id": self.id,
            "at": self.at,
            "host": self.host,
            "kind": self.kind.as_str(),
            "summary": self.summary(),
            "link": self.link,
            "alerts": alerts,
        })
    }

    /// A one-line summary. Names no object, so it is safe for a minimal push.
    pub fn summary(&self) -> String {
        match &self.kind {
            MessageKind::Alerts => {
                let critical = self
                    .notices
                    .iter()
                    .filter(|n| n.severity == "critical")
                    .count();
                let first = self.notices.first().map_or("", |n| n.text.title_en);
                format!(
                    "{} alert(s), {critical} critical: {first}",
                    self.notices.len()
                )
            }
            MessageKind::Digest { skipped } => format!(
                "{skipped} more alert(s) were not sent one by one this hour; see the admin page"
            ),
            MessageKind::Test => "test message from the mailrs ops daemon".into(),
            MessageKind::MaintenanceEnded { held } => {
                format!("maintenance ended; {held} notice(s) were held back during it")
            }
        }
    }

    /// The push title and body. Minimal: rule names, levels and the kind of
    /// object only; no host, object, value, reason or link (the link names
    /// the server's own domain). The full form carries the link.
    pub fn push_text(&self, minimal: bool) -> (String, String) {
        let title = match minimal {
            true => format!("mailrs: {}", self.summary()),
            false => format!("mailrs {}: {}", self.host, self.summary()),
        };
        let mut body: Vec<String> = self.notices.iter().map(|n| n.text_line(minimal)).collect();
        if let (false, Some(link)) = (minimal, &self.link) {
            body.push(link.clone());
        }
        (title, body.join("\n"))
    }

    /// Whether any notice is critical: the push priority.
    pub fn critical(&self) -> bool {
        self.notices.iter().any(|n| n.severity == "critical")
    }

    /// The relay mail's subject and plain-text body, in full.
    pub fn mail_text(&self) -> (String, String) {
        let subject = format!("[mailrs {}] {}", self.host, self.summary());
        let mut body = String::new();
        for n in &self.notices {
            body.push_str(&format!(
                "{} — {} / {}\r\n{}\r\nobject: {} ({})\r\nvalue: {}  threshold: {}\r\nsince: {}\r\n\r\n{}\r\n{}\r\n\r\nFirst step: {}\r\n第一步：{}\r\n\r\n",
                n.kind,
                n.text.title_en,
                n.text.title_zh,
                n.severity,
                n.object,
                n.object_kind,
                n.value,
                n.threshold,
                chrono::DateTime::from_timestamp(n.since, 0).map_or(n.since.to_string(), |d| d.to_rfc3339()),
                n.text.meaning_en,
                n.text.meaning_zh,
                n.text.step_en,
                n.text.step_zh,
            ));
        }
        if self.notices.is_empty() {
            body.push_str(&self.summary());
            body.push_str("\r\n");
        }
        if let Some(link) = &self.link {
            body.push_str(link);
            body.push_str("\r\n");
        }
        (subject, body)
    }
}
