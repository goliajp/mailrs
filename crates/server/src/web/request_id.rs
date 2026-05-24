use std::time::Instant;

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use rand_core::{OsRng, RngCore};

const REQUEST_ID_HEADER: &str = "x-request-id";

/// generate a random 16-byte hex string (32 chars) as request id
fn generate_request_id() -> String {
    let mut buf = [0u8; 16];
    OsRng.fill_bytes(&mut buf);
    hex_encode(&buf)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize]);
        s.push(HEX_CHARS[(b & 0x0f) as usize]);
    }
    s
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// validate that a client-provided request id is safe to use
fn is_valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// middleware that assigns a request id and logs request details
pub async fn request_id_middleware(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // reuse client-provided request id if valid, otherwise generate one
    let request_id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| is_valid_request_id(v))
        .map(String::from)
        .unwrap_or_else(generate_request_id);

    let mut response = next.run(req).await;

    let duration = start.elapsed();
    let status = response.status().as_u16();

    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        status = status,
        duration_ms = duration.as_millis() as u64,
        "request completed"
    );

    if let Ok(header_value) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER, header_value);
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_request_id_length() {
        let id = generate_request_id();
        assert_eq!(id.len(), 32);
    }

    #[test]
    fn generate_request_id_is_hex() {
        let id = generate_request_id();
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_request_id_unique() {
        let a = generate_request_id();
        let b = generate_request_id();
        assert_ne!(a, b);
    }

    #[test]
    fn hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn hex_encode_single_byte() {
        assert_eq!(hex_encode(&[0xff]), "ff");
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0x0a]), "0a");
    }

    #[test]
    fn hex_encode_multiple_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn is_valid_request_id_alphanumeric() {
        assert!(is_valid_request_id("abc123"));
    }

    #[test]
    fn is_valid_request_id_with_special_chars() {
        assert!(is_valid_request_id("abc-123_456.789"));
    }

    #[test]
    fn is_valid_request_id_empty() {
        assert!(!is_valid_request_id(""));
    }

    #[test]
    fn is_valid_request_id_too_long() {
        let long = "a".repeat(129);
        assert!(!is_valid_request_id(&long));
    }

    #[test]
    fn is_valid_request_id_max_length() {
        let max = "a".repeat(128);
        assert!(is_valid_request_id(&max));
    }

    #[test]
    fn is_valid_request_id_rejects_spaces() {
        assert!(!is_valid_request_id("abc 123"));
    }

    #[test]
    fn is_valid_request_id_rejects_newlines() {
        assert!(!is_valid_request_id("abc\n123"));
    }

    #[test]
    fn is_valid_request_id_rejects_unicode() {
        assert!(!is_valid_request_id("日本語"));
    }

    #[test]
    fn is_valid_request_id_rejects_slashes() {
        assert!(!is_valid_request_id("abc/123"));
    }

    #[test]
    fn is_valid_request_id_rejects_colons() {
        assert!(!is_valid_request_id("abc:123"));
    }
}
