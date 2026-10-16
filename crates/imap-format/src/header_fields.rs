//! `BODY[HEADER.FIELDS (…)]` and `BODY[HEADER.FIELDS.NOT (…)]`: a
//! message's header block cut down to the named fields, or to all but
//! them.

use crate::mime::extract_header_section;

/// extract only the specified header fields from raw message
pub fn extract_header_fields(data: &[u8], fields: &[String]) -> Vec<u8> {
    select_header_fields(data, |name| fields.iter().any(|f| f == name))
}

/// every header field except the specified ones (`HEADER.FIELDS.NOT`)
pub fn extract_header_fields_except(data: &[u8], fields: &[String]) -> Vec<u8> {
    select_header_fields(data, |name| !fields.iter().any(|f| f == name))
}

// `keep` sees the field name uppercased; a continuation line follows its
// field's fate
fn select_header_fields(data: &[u8], keep: impl Fn(&str) -> bool) -> Vec<u8> {
    let header = extract_header_section(data);
    let header_str = String::from_utf8_lossy(&header);
    let mut result = Vec::new();
    let mut include = false;
    for line in header_str.lines() {
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if include {
                result.extend_from_slice(line.as_bytes());
                result.extend_from_slice(b"\r\n");
            }
        } else {
            include = false;
            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim().to_uppercase();
                if keep(&name) {
                    include = true;
                    result.extend_from_slice(line.as_bytes());
                    result.extend_from_slice(b"\r\n");
                }
            }
        }
    }
    result.extend_from_slice(b"\r\n");
    result
}
