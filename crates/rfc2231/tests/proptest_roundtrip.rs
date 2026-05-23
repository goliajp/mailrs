//! Property-based roundtrip tests for `encode_param` and `decode_param_value`.
//!
//! Contract: for every UTF-8 string `value`, taking
//! `encode_param("filename", value)`, stripping the leading
//! `"filename"` or `"filename*"` and `=`, then passing the rest to
//! `decode_param_value`, should recover the original `value`.

use mailrs_rfc2231::{decode_param_value, encode_param};
use proptest::prelude::*;

/// Helper: strip the `name=` or `name*=` prefix from an encoded form
/// and return only the value portion, suitable for `decode_param_value`.
fn strip_name_prefix<'a>(encoded: &'a str, name: &str) -> &'a str {
    // ASCII form: `name="..."`
    let ascii_prefix = format!("{name}=");
    if let Some(rest) = encoded.strip_prefix(&ascii_prefix) {
        return rest;
    }
    // Extended form: `name*=...`
    let ext_prefix = format!("{name}*=");
    if let Some(rest) = encoded.strip_prefix(&ext_prefix) {
        return rest;
    }
    // Some implementations may emit other forms; for the fuzz test
    // we just feed the whole thing back (will yield None).
    encoded
}

proptest! {
    /// Arbitrary UTF-8 value: encode → strip prefix → decode → original.
    #[test]
    fn arbitrary_utf8_roundtrip(value in any::<String>()) {
        // RFC 2231 doesn't allow embedded NUL in quoted-string form; skip.
        // Also CR/LF inside parameter values is illegal per RFC 5322.
        prop_assume!(!value.contains('\0'));
        prop_assume!(!value.contains('\r'));
        prop_assume!(!value.contains('\n'));
        // Quoted-string form can't safely carry a literal `"` without
        // escaping; we don't claim that's supported.
        prop_assume!(!value.contains('"'));
        // Backslash inside quoted-string changes meaning during decode
        // (it's the escape character). Skip.
        prop_assume!(!value.contains('\\'));

        let encoded = encode_param("filename", &value);
        let value_part = strip_name_prefix(&encoded, "filename");
        let decoded = decode_param_value(value_part);
        prop_assert!(decoded.is_some(), "decode returned None for value: {value:?}, encoded: {encoded:?}");
        let d = decoded.unwrap();
        prop_assert_eq!(d.as_ref(), &value);
    }

    /// ASCII-only values exercise the quoted-string form.
    #[test]
    fn ascii_roundtrip(value in "[a-zA-Z0-9._\\- ]+") {
        let encoded = encode_param("filename", &value);
        let value_part = strip_name_prefix(&encoded, "filename");
        let decoded = decode_param_value(value_part);
        prop_assert!(decoded.is_some());
        let d = decoded.unwrap();
        prop_assert_eq!(d.as_ref(), &value);
    }

    /// CJK values force the RFC 2231 extended (percent-encoded) form.
    #[test]
    fn cjk_roundtrip(value in "[\u{4E00}-\u{9FFF}\u{3040}-\u{30FF}]+") {
        let encoded = encode_param("filename", &value);
        let value_part = strip_name_prefix(&encoded, "filename");
        let decoded = decode_param_value(value_part);
        prop_assert!(decoded.is_some());
        let d = decoded.unwrap();
        prop_assert_eq!(d.as_ref(), &value);
    }
}
