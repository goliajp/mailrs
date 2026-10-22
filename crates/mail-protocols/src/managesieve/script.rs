//! The literal a client sends a script or a SASL response in.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

use super::SCRIPT_NAME;

/// Read the bytes of a `{n}` / `{n+}` literal whose marker was `{spec}`,
/// and the line end after them.
pub(super) async fn read_literal<R>(spec: &str, reader: &mut R) -> std::io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let len: usize = spec.trim_end_matches('+').parse().unwrap_or(0);
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let mut tail = String::new();
    reader.read_line(&mut tail).await?;
    Ok(Some(buf))
}

/// The script name and body of PUTSCRIPT / CHECKSCRIPT, whose `arg` is
/// `"name" {len+}` or just `{len+}`. `None` when the literal is refused.
pub(super) async fn read_script_literal<R>(
    arg: &str,
    reader: &mut R,
) -> std::io::Result<Option<(String, String)>>
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
    Ok(body.map(|b| (name, String::from_utf8_lossy(&b).into_owned())))
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
