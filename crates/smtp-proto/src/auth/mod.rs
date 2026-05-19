//! SASL helpers for AUTH PLAIN ([RFC 4616]) and AUTH LOGIN.
//!
//! These functions decode SASL payloads. They do not verify credentials —
//! the caller is responsible for looking up usernames and checking passwords.
//!
//! [RFC 4616]: https://datatracker.ietf.org/doc/html/rfc4616

use base64::{Engine, engine::general_purpose::STANDARD};

/// Error returned by [`decode_plain`] / [`decode_login_response`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Payload was not valid base64.
    InvalidBase64,
    /// AUTH PLAIN payload did not contain two NUL separators.
    MalformedPayload,
    /// Username field is empty.
    EmptyUsername,
    /// Password field is empty.
    EmptyPassword,
}

/// Decode an AUTH PLAIN payload (`base64(authzid \0 authcid \0 passwd)`).
/// Returns `(username, password)`; the authzid field is ignored.
pub fn decode_plain(encoded: &str) -> Result<(String, String), AuthError> {
    let bytes = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| AuthError::InvalidBase64)?;

    // find the two NUL separators
    let first_null = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or(AuthError::MalformedPayload)?;
    let rest = &bytes[first_null + 1..];
    let second_null = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(AuthError::MalformedPayload)?;

    let username = &rest[..second_null];
    let password = &rest[second_null + 1..];

    if username.is_empty() {
        return Err(AuthError::EmptyUsername);
    }
    if password.is_empty() {
        return Err(AuthError::EmptyPassword);
    }

    Ok((
        String::from_utf8_lossy(username).into_owned(),
        String::from_utf8_lossy(password).into_owned(),
    ))
}

/// Decode a single AUTH LOGIN continuation line (username or password,
/// base64-encoded).
pub fn decode_login_response(encoded: &str) -> Result<String, AuthError> {
    let bytes = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| AuthError::InvalidBase64)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests;
