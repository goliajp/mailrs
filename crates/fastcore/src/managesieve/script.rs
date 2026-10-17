//! The one script a user has, in the network kevy, and the literal a
//! client sends it in.

use kevy_client::Connection;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

use super::SCRIPT_NAME;

/// Parse the `{n}` / `{n+}` literal that follows PUTSCRIPT/CHECKSCRIPT
/// and read exactly n bytes. `arg` is `"name" {len+}` or just `{len+}`.
pub(super) async fn read_script_literal<R>(
    arg: &str,
    reader: &mut R,
) -> std::io::Result<(String, String)>
where
    R: AsyncBufRead + Unpin,
{
    // extract optional script name + the {len} literal marker
    let name = if arg.trim_start().starts_with('"') {
        let inner = &arg.trim_start()[1..];
        inner.split('"').next().unwrap_or("").to_string()
    } else {
        SCRIPT_NAME.to_string()
    };
    let len: usize = arg
        .rsplit('{')
        .next()
        .and_then(|s| s.trim_end_matches(['}', '+', '\r', '\n']).parse().ok())
        .unwrap_or(0);
    let mut buf = vec![0u8; len];
    tokio::io::AsyncReadExt::read_exact(reader, &mut buf).await?;
    // consume the trailing CRLF after the literal payload
    let mut tail = String::new();
    let _ = reader.read_line(&mut tail).await;
    Ok((name, String::from_utf8_lossy(&buf).into_owned()))
}

fn kevy() -> Option<Connection> {
    let url = std::env::var("MAILRS_KEVY_URL").ok()?;
    Connection::connect(&url).ok()
}

pub(super) fn store_script(user: &str, body: &str) -> bool {
    let Some(mut c) = kevy() else { return false };
    c.set(
        mailrs_core_sidestate::sieve_key(user).as_bytes(),
        body.as_bytes(),
    )
    .is_ok()
}

pub(super) fn load_script(user: &str) -> Option<String> {
    let mut c = kevy()?;
    let raw = c
        .get(mailrs_core_sidestate::sieve_key(user).as_bytes())
        .ok()
        .flatten()?;
    let s = String::from_utf8(raw).ok()?;
    (!s.trim().is_empty()).then_some(s)
}

pub(super) fn delete_script(user: &str) {
    if let Some(mut c) = kevy() {
        let _ = c.del(&[mailrs_core_sidestate::sieve_key(user).as_bytes()]);
    }
}

/// ManageSieve response strings are quoted; strip CR/LF/quote so a
/// compiler error can't break the wire framing.
pub(super) fn sanitize(s: &str) -> String {
    s.replace(['\r', '\n', '"'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_neutralizes_framing_chars() {
        assert_eq!(sanitize("bad\r\nscript\"x"), "bad  script x");
    }
}
