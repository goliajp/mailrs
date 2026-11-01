//! What a Word, Excel or PowerPoint attachment says, for reading it in place
//! rather than downloading it.

use mailrs_attachment_extract::office::{Section, office_kind, office_text};

/// At most this much text comes back; a longer file says it was cut.
const MAX_CHARS: usize = 200_000;

/// The preview of an Office attachment, or `None` when it is not one.
pub(super) fn office(part: &mailrs_mime::Part<'_>) -> Option<serde_json::Value> {
    let name = part
        .disposition
        .as_ref()
        .and_then(|d| d.filename())
        .or_else(|| part.content_type.name())
        .unwrap_or("");
    let kind = office_kind(&part.content_type.mime_type(), name)?;
    let content_type = part.content_type.mime_type();
    Some(match office_text(kind, &part.body) {
        Ok(sections) => {
            let (text, cut) = flatten(&sections);
            serde_json::json!({
                "success": !text.trim().is_empty(),
                "extracted_text": text,
                "truncated": cut,
                "sections": sections,
                "content_type": content_type,
            })
        }
        Err(e) => {
            tracing::info!(error = %e, "office attachment could not be read");
            serde_json::json!({
                "success": false,
                "extracted_text": "",
                "content_type": content_type,
            })
        }
    })
}

/// The sections as one text, a titled section under its title, capped.
fn flatten(sections: &[Section]) -> (String, bool) {
    let mut out = String::new();
    for s in sections {
        if !s.title.is_empty() {
            out.push_str(&format!("— {} —\n", s.title));
        }
        for line in &s.lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    match out.char_indices().nth(MAX_CHARS) {
        Some((at, _)) => (out[..at].to_string(), true),
        None => (out.trim_end().to_string(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_flatten_under_their_titles() {
        let sections = vec![
            Section {
                title: "Q1".into(),
                lines: vec!["a\tb".into()],
            },
            Section {
                title: String::new(),
                lines: vec!["plain".into()],
            },
        ];
        assert_eq!(
            flatten(&sections),
            ("— Q1 —\na\tb\n\nplain".to_string(), false)
        );
    }

    #[test]
    fn a_long_text_is_cut_and_says_so() {
        let sections = vec![Section {
            title: String::new(),
            lines: vec!["x".repeat(MAX_CHARS + 10)],
        }];
        let (text, cut) = flatten(&sections);
        assert!(cut);
        assert_eq!(text.chars().count(), MAX_CHARS);
    }
}
