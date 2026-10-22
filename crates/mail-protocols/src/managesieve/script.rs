//! The literal a client sends a script or a SASL response in.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use super::SCRIPT_NAME;

/// The largest literal accepted. The size is the client's to declare and
/// the buffer is taken before the bytes arrive, before sign-in too.
pub(super) const MAX_LITERAL: usize = 1 << 20;

/// Read the bytes of a `{n}` / `{n+}` literal whose marker was `{spec}`,
/// and the line end after them. A literal over [`MAX_LITERAL`] is an
/// error, which ends the connection: its bytes are already on the way.
pub(super) async fn read_literal<R>(spec: &str, reader: &mut R) -> std::io::Result<Vec<u8>>
where
    R: AsyncBufRead + Unpin,
{
    let len: usize = spec.trim_end_matches('+').parse().unwrap_or(0);
    if len > MAX_LITERAL {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("literal of {len} bytes refused"),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let mut tail = String::new();
    reader.read_line(&mut tail).await?;
    Ok(buf)
}

/// The script name and body of PUTSCRIPT / CHECKSCRIPT, whose `arg` is
/// `"name" {len+}` or just `{len+}`.
pub(super) async fn read_script_literal<R>(
    arg: &str,
    reader: &mut R,
) -> std::io::Result<(String, String)>
where
    R: AsyncBufRead + Unpin,
{
    let name = match arg.trim_start().strip_prefix('"') {
        Some(inner) => inner.split('"').next().unwrap_or("").to_string(),
        None => SCRIPT_NAME.to_string(),
    };
    let spec = arg
        .rsplit('{')
        .next()
        .unwrap_or("")
        .trim_end_matches(['}', '\r', '\n']);
    let body = read_literal(spec, reader).await?;
    Ok((name, String::from_utf8_lossy(&body).into_owned()))
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

    #[tokio::test]
    async fn a_literal_over_the_limit_is_refused_before_any_buffer_is_taken() {
        let mut reader = tokio::io::BufReader::new(&b"keep;\r\n"[..]);
        let e = read_literal("99999999999999+", &mut reader)
            .await
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        let at_limit = format!("{MAX_LITERAL}+");
        let e = read_literal(&at_limit, &mut reader).await.unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof, "{e}");
    }
}
