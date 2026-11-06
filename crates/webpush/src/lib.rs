//! Web Push for "you have mail": the message encrypted to the browser's
//! keys (RFC 8291), signed for the push service (RFC 8292), and sent only
//! to a known browser push service whose addresses are public. A phone's
//! UnifiedPush endpoint takes the same request, to its distributor's host.

pub mod ece;
pub mod endpoint;
pub mod send;
pub mod vapid;

use base64::Engine as _;

/// base64url without padding, the encoding every Web Push value uses.
pub fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The inverse of [`b64url`]; padding and the standard alphabet accepted,
/// as browsers are not consistent about either.
pub fn from_b64url(text: &str) -> Option<Vec<u8>> {
    let clean: String = text
        .trim()
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            other => other,
        })
        .collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(clean)
        .ok()
}
