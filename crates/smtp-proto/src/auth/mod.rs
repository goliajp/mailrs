use base64::{engine::general_purpose::STANDARD, Engine};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    InvalidBase64,
    MalformedPayload,
    EmptyUsername,
    EmptyPassword,
}

/// decode AUTH PLAIN payload: base64(authzid \0 authcid \0 passwd)
/// returns (username, password) — authzid is ignored
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

/// decode a single AUTH LOGIN response (username or password, base64-encoded)
pub fn decode_login_response(encoded: &str) -> Result<String, AuthError> {
    let bytes = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| AuthError::InvalidBase64)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests;
