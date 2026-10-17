//! AUTHENTICATE PLAIN (RFC 4616) against the account store IMAP and
//! POP3 use.

use std::sync::Arc;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use super::unquote;
use crate::FastcoreState;

/// AUTHENTICATE PLAIN: the initial response inline, or sent after an
/// empty challenge. Returns the authenticated address on success.
pub(super) async fn handle_authenticate<R, W>(
    state: &Arc<FastcoreState>,
    arg: &str,
    reader: &mut R,
    tx: &mut W,
) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // arg = `"PLAIN"` optionally followed by the initial response
    let mut it = arg.splitn(2, ' ');
    let mech = unquote(it.next().unwrap_or(""));
    if !mech.eq_ignore_ascii_case("PLAIN") {
        return Ok(None);
    }
    let b64 = match it.next() {
        Some(rest) if !rest.trim().is_empty() => unquote(rest.trim()).to_string(),
        _ => {
            // PLAIN has an empty challenge (RFC 5804 §2.1)
            tx.write_all(b"\"\"\r\n").await?;
            match client_response(reader).await? {
                Some(resp) => resp,
                None => return Ok(None),
            }
        }
    };
    use base64::Engine as _;
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) else {
        return Ok(None);
    };
    // SASL PLAIN: authzid \0 authcid \0 passwd
    let mut fields = raw.split(|b| *b == 0);
    let _authzid = fields.next();
    let authcid = fields.next().and_then(|b| std::str::from_utf8(b).ok());
    let passwd = fields.next().and_then(|b| std::str::from_utf8(b).ok());
    match (authcid, passwd) {
        (Some(user), Some(pass)) if crate::imap::backend::verify_password(state, user, pass) => {
            Ok(Some(user.to_string()))
        }
        _ => Ok(None),
    }
}

/// The client's answer to a challenge: a quoted string or a `{n}` / `{n+}`
/// literal holding the base64 response. `None` when it cancels with "*".
async fn client_response<R>(reader: &mut R) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let line = line.trim_end_matches(['\r', '\n']).trim();
    let resp = match line.strip_prefix('{').and_then(|l| l.strip_suffix('}')) {
        Some(len) => {
            let len: usize = len.trim_end_matches('+').parse().unwrap_or(0);
            let mut buf = vec![0u8; len];
            tokio::io::AsyncReadExt::read_exact(reader, &mut buf).await?;
            let mut tail = String::new();
            reader.read_line(&mut tail).await?;
            String::from_utf8_lossy(&buf).into_owned()
        }
        None => unquote(line).to_string(),
    };
    Ok((resp != "*").then_some(resp))
}

#[cfg(test)]
mod tests {
    use super::client_response;

    async fn answer(input: &str) -> Option<String> {
        let mut reader = tokio::io::BufReader::new(input.as_bytes());
        client_response(&mut reader).await.unwrap()
    }

    #[tokio::test]
    async fn a_response_is_read_as_a_string_or_a_literal() {
        assert_eq!(
            answer("\"AGFAYgBj\"\r\n").await.as_deref(),
            Some("AGFAYgBj")
        );
        assert_eq!(
            answer("{8+}\r\nAGFAYgBj\r\n").await.as_deref(),
            Some("AGFAYgBj")
        );
        assert_eq!(
            answer("{8}\r\nAGFAYgBj\r\n").await.as_deref(),
            Some("AGFAYgBj")
        );
        assert_eq!(answer("\"*\"\r\n").await, None);
    }
}
