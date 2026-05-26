//! RFC 9051 §7.5.2 BODYSTRUCTURE assembly.

use crate::escape_imap_str;
use crate::mime::{
    extract_body_section, extract_header_section, parse_mime_headers, split_mime_parts,
    trim_part_trailing_newline,
};

/// build a single part's BODYSTRUCTURE string (with extension data)
fn build_part_bodystructure(part_data: &[u8]) -> String {
    let header = extract_header_section(part_data);
    let header_str = String::from_utf8_lossy(&header);
    let body = extract_body_section(part_data);
    let info = parse_mime_headers(&header_str);

    if info.media_type == "MULTIPART" {
        if let Some(ref boundary) = info.boundary {
            let parts = split_mime_parts(&body, boundary);
            let parts_str: String = parts.iter().map(|p| build_part_bodystructure(p)).collect();
            return format!(
                "({} \"{}\" (\"boundary\" \"{}\") NIL NIL)",
                parts_str,
                info.subtype.to_lowercase(),
                boundary,
            );
        }
        let body_trimmed = trim_part_trailing_newline(&body);
        let body_lines = body_trimmed.split(|&b| b == b'\n').count();
        return format!(
            "(\"text\" \"plain\" (\"charset\" \"UTF-8\") NIL NIL \"7bit\" {} {} NIL NIL NIL)",
            body_trimmed.len(),
            body_lines,
        );
    }

    let body_trimmed = trim_part_trailing_newline(&body);

    let params = {
        let mut pairs = Vec::new();
        if info.media_type == "TEXT" {
            pairs.push(format!("\"charset\" \"{}\"", info.charset));
        }
        if let Some(ref n) = info.name {
            pairs.push(format!("\"name\" \"{}\"", escape_imap_str(n)));
        }
        if pairs.is_empty() {
            "NIL".to_string()
        } else {
            format!("({})", pairs.join(" "))
        }
    };

    let cid = info
        .content_id
        .as_ref()
        .map(|id| format!("\"<{}>\"", id))
        .unwrap_or_else(|| "NIL".to_string());

    let dsp = if let Some(ref disp) = info.disposition {
        if let Some(fname) = info.disposition_filename.as_ref().or(info.name.as_ref()) {
            format!(
                "(\"{}\" (\"filename\" \"{}\"))",
                disp,
                escape_imap_str(fname)
            )
        } else {
            format!("(\"{}\" NIL)", disp)
        }
    } else {
        "NIL".to_string()
    };

    if info.media_type == "TEXT" {
        let body_lines = body_trimmed.split(|&b| b == b'\n').count();
        format!(
            "(\"text\" \"{}\" {} {} NIL \"{}\" {} {} NIL {} NIL)",
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body_trimmed.len(),
            body_lines,
            dsp,
        )
    } else {
        format!(
            "(\"{}\" \"{}\" {} {} NIL \"{}\" {} NIL {} NIL)",
            info.media_type.to_lowercase(),
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body_trimmed.len(),
            dsp,
        )
    }
}

/// build BODYSTRUCTURE for a message (handles multipart, with extension data)
pub fn build_bodystructure(data: &[u8]) -> String {
    let header_bytes = extract_header_section(data);
    let header = String::from_utf8_lossy(&header_bytes);
    let body = extract_body_section(data);
    let info = parse_mime_headers(&header);

    if info.media_type == "MULTIPART"
        && let Some(ref boundary) = info.boundary
    {
        let parts = split_mime_parts(&body, boundary);
        if !parts.is_empty() {
            let parts_str: String = parts.iter().map(|p| build_part_bodystructure(p)).collect();
            return format!(
                "({} \"{}\" (\"boundary\" \"{}\") NIL NIL)",
                parts_str,
                info.subtype.to_lowercase(),
                boundary,
            );
        }
    }

    let params = {
        let mut pairs = Vec::new();
        if info.media_type == "TEXT" {
            pairs.push(format!("\"charset\" \"{}\"", info.charset));
        }
        if let Some(ref n) = info.name {
            pairs.push(format!("\"name\" \"{}\"", escape_imap_str(n)));
        }
        if pairs.is_empty() {
            "NIL".to_string()
        } else {
            format!("({})", pairs.join(" "))
        }
    };
    let cid = info
        .content_id
        .as_ref()
        .map(|id| format!("\"<{}>\"", id))
        .unwrap_or_else(|| "NIL".to_string());
    let dsp = if let Some(ref disp) = info.disposition {
        if let Some(fname) = info.disposition_filename.as_ref().or(info.name.as_ref()) {
            format!(
                "(\"{}\" (\"filename\" \"{}\"))",
                disp,
                escape_imap_str(fname)
            )
        } else {
            format!("(\"{}\" NIL)", disp)
        }
    } else {
        "NIL".to_string()
    };

    if info.media_type == "TEXT" {
        let body_lines = body.split(|&b| b == b'\n').count();
        format!(
            "(\"text\" \"{}\" {} {} NIL \"{}\" {} {} NIL {} NIL)",
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body.len(),
            body_lines,
            dsp,
        )
    } else {
        format!(
            "(\"{}\" \"{}\" {} {} NIL \"{}\" {} NIL {} NIL)",
            info.media_type.to_lowercase(),
            info.subtype.to_lowercase(),
            params,
            cid,
            info.encoding.to_lowercase(),
            body.len(),
            dsp,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_bodystructure_simple_text() {
        let msg = b"Content-Type: text/plain; charset=UTF-8\r\n\r\nHello world";
        let bs = build_bodystructure(msg);
        assert!(bs.contains("\"text\""));
        assert!(bs.contains("\"plain\""));
        assert!(bs.contains("\"charset\" \"UTF-8\""));
    }

    #[test]
    fn build_bodystructure_multipart_alternative() {
        let body = concat!(
            "Content-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n",
            "--alt\r\n",
            "Content-Type: text/plain; charset=UTF-8\r\n\r\n",
            "plain text\r\n",
            "--alt\r\n",
            "Content-Type: text/html; charset=UTF-8\r\n\r\n",
            "<p>html</p>\r\n",
            "--alt--\r\n",
        )
        .as_bytes();
        let bs = build_bodystructure(body);
        assert!(bs.contains("\"alternative\""));
        assert!(bs.contains("\"plain\""));
        assert!(bs.contains("\"html\""));
    }
}
