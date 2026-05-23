//! Content-Type + Content-Disposition header value parsing.
//!
//! Goes only as deep as we need for the MIME body tree walker: split
//! `type/subtype` and pick out a `boundary=` / `charset=` / `filename=`
//! / `name=` parameter. RFC 2231 extended forms (`filename*=...`) are
//! decoded via [`mailrs-rfc2231`](mailrs_rfc2231).

use std::collections::HashMap;

/// Parsed `Content-Type:` header value.
#[derive(Debug, Clone)]
pub struct ContentType {
    /// Top-level type ("text", "multipart", "application", ...), lowercased.
    pub type_: String,
    /// Subtype ("plain", "html", "alternative", "mixed", "calendar", ...),
    /// lowercased.
    pub subtype: String,
    /// Lowercased parameter map (boundary, charset, name, ...).
    /// Values are RFC 2231-decoded when the on-wire shape was
    /// `name*=charset''pct-encoded`.
    pub params: HashMap<String, String>,
}

impl ContentType {
    /// Default per RFC 2045 §5.2 when no Content-Type is present:
    /// `text/plain; charset=us-ascii`.
    pub fn default_for_missing_header() -> Self {
        let mut params = HashMap::new();
        params.insert("charset".into(), "us-ascii".into());
        Self {
            type_: "text".into(),
            subtype: "plain".into(),
            params,
        }
    }

    /// Parse from raw header value (e.g. `multipart/mixed; boundary="xyz"`).
    pub fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        // type/subtype is everything up to the first `;`.
        let (kind, rest) = match trimmed.split_once(';') {
            Some((k, r)) => (k.trim(), r),
            None => (trimmed, ""),
        };
        let (type_, subtype) = match kind.split_once('/') {
            Some((t, s)) => (t.trim().to_ascii_lowercase(), s.trim().to_ascii_lowercase()),
            None => (kind.to_ascii_lowercase(), String::new()),
        };
        let params = parse_params(rest);
        Self {
            type_,
            subtype,
            params,
        }
    }

    /// `true` if this is a multipart/* type.
    pub fn is_multipart(&self) -> bool {
        self.type_ == "multipart"
    }

    /// Convenience: `"<type>/<subtype>"`.
    pub fn mime_type(&self) -> String {
        format!("{}/{}", self.type_, self.subtype)
    }

    /// `boundary=` parameter for multipart parts. `None` for non-multipart
    /// or malformed multipart.
    pub fn boundary(&self) -> Option<&str> {
        self.params.get("boundary").map(String::as_str)
    }

    /// `charset=` parameter for text/* parts. Defaults to "us-ascii"
    /// per RFC 2045 §5.2 when absent.
    pub fn charset(&self) -> &str {
        self.params
            .get("charset")
            .map(String::as_str)
            .unwrap_or("us-ascii")
    }

    /// `name=` parameter — historical attachment filename source.
    /// See also Content-Disposition `filename=`.
    pub fn name(&self) -> Option<&str> {
        self.params.get("name").map(String::as_str)
    }
}

/// Parsed `Content-Disposition:` header value.
#[derive(Debug, Clone)]
pub struct Disposition {
    /// `"inline"`, `"attachment"`, or other disposition type, lowercased.
    pub kind: String,
    /// Same shape as `ContentType::params`.
    pub params: HashMap<String, String>,
}

impl Disposition {
    /// Parse from raw header value (e.g.
    /// `attachment; filename="report.pdf"`).
    pub fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        let (kind, rest) = match trimmed.split_once(';') {
            Some((k, r)) => (k.trim().to_ascii_lowercase(), r),
            None => (trimmed.to_ascii_lowercase(), ""),
        };
        let params = parse_params(rest);
        Self { kind, params }
    }

    /// `filename=` parameter (RFC 2183) — preferred attachment name.
    pub fn filename(&self) -> Option<&str> {
        self.params.get("filename").map(String::as_str)
    }

    /// `true` if `kind == "attachment"`.
    pub fn is_attachment(&self) -> bool {
        self.kind == "attachment"
    }

    /// `true` if `kind == "inline"`.
    pub fn is_inline(&self) -> bool {
        self.kind == "inline"
    }
}

/// Parse the `; name=value; name2=value2` parameter tail of a
/// Content-Type / Content-Disposition header value.
///
/// Handles both legacy quoted (`name="value"`) and RFC 2231 extended
/// (`name*=UTF-8''pct-encoded`) forms via [`mailrs-rfc2231`].
fn parse_params(input: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    // Naive split on `;`. Values may legitimately contain `;` inside
    // quoted strings — RFC-strict parsing would tokenize MIME-style.
    // We accept the simple split; if a quoted value contains `;` we'd
    // truncate, which is rare in practice.
    for token in input.split(';') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let Some((name, value)) = token.split_once('=') else {
            continue;
        };
        let mut name = name.trim().to_ascii_lowercase();
        // RFC 2231 extended form: `filename*=UTF-8''...` — trailing
        // `*` marks the value as extended-form. Strip it so callers
        // look up by the base name (`filename`, not `filename*`).
        if let Some(base) = name.strip_suffix('*') {
            name = base.to_string();
        }
        let value_decoded = mailrs_rfc2231::decode_param_value(value.trim())
            .map(|c| c.into_owned())
            .unwrap_or_else(|| value.trim().to_string());
        // Trim quotes if the value came back quoted but decode didn't
        // strip them (fallback path).
        let value_clean = value_decoded
            .trim()
            .trim_matches('"')
            .to_string();
        out.insert(name, value_clean);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_text_plain() {
        let ct = ContentType::parse("text/plain");
        assert_eq!(ct.type_, "text");
        assert_eq!(ct.subtype, "plain");
        assert!(ct.params.is_empty());
    }

    #[test]
    fn parse_text_plain_with_charset() {
        let ct = ContentType::parse("text/plain; charset=utf-8");
        assert_eq!(ct.charset(), "utf-8");
    }

    #[test]
    fn parse_multipart_with_boundary() {
        let ct = ContentType::parse("multipart/mixed; boundary=\"abc-123\"");
        assert!(ct.is_multipart());
        assert_eq!(ct.boundary(), Some("abc-123"));
    }

    #[test]
    fn parse_multipart_unquoted_boundary() {
        let ct = ContentType::parse("multipart/alternative; boundary=xyz");
        assert_eq!(ct.boundary(), Some("xyz"));
    }

    #[test]
    fn parse_case_insensitive_type() {
        let ct = ContentType::parse("TEXT/HTML");
        assert_eq!(ct.type_, "text");
        assert_eq!(ct.subtype, "html");
    }

    #[test]
    fn parse_attachment_filename_quoted() {
        let ct = ContentType::parse("application/pdf; name=\"report.pdf\"");
        assert_eq!(ct.name(), Some("report.pdf"));
    }

    #[test]
    fn parse_rfc2231_filename_decoded() {
        let ct = ContentType::parse(
            "application/pdf; name*=UTF-8''%E6%97%A5%E6%9C%AC.pdf",
        );
        assert_eq!(ct.name(), Some("日本.pdf"));
    }

    #[test]
    fn parse_disposition_attachment() {
        let d = Disposition::parse("attachment; filename=\"report.pdf\"");
        assert!(d.is_attachment());
        assert_eq!(d.filename(), Some("report.pdf"));
    }

    #[test]
    fn parse_disposition_inline() {
        let d = Disposition::parse("inline");
        assert!(d.is_inline());
        assert!(d.filename().is_none());
    }

    #[test]
    fn parse_disposition_rfc2231_filename() {
        let d = Disposition::parse("attachment; filename*=UTF-8''%E6%97%A5.pdf");
        assert_eq!(d.filename(), Some("日.pdf"));
    }

    #[test]
    fn default_for_missing_header_is_text_plain_ascii() {
        let ct = ContentType::default_for_missing_header();
        assert_eq!(ct.mime_type(), "text/plain");
        assert_eq!(ct.charset(), "us-ascii");
    }

    #[test]
    fn parse_handles_extra_whitespace() {
        let ct = ContentType::parse("  multipart/mixed ;  boundary=\"xx\"  ");
        assert!(ct.is_multipart());
        assert_eq!(ct.boundary(), Some("xx"));
    }

    #[test]
    fn parse_no_subtype_yields_empty() {
        let ct = ContentType::parse("application");
        assert_eq!(ct.type_, "application");
        assert_eq!(ct.subtype, "");
    }

    #[test]
    fn parse_handles_multiple_params() {
        let ct = ContentType::parse(
            "text/plain; charset=utf-8; format=flowed; delsp=yes",
        );
        assert_eq!(ct.charset(), "utf-8");
        assert_eq!(ct.params.get("format").map(String::as_str), Some("flowed"));
        assert_eq!(ct.params.get("delsp").map(String::as_str), Some("yes"));
    }
}
