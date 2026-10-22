//! APPEND (RFC 3501 §6.3.11): the literal is read here, not by the
//! command parser.

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::ImapInput;
use mailrs_imap_proto::{format_no, format_ok};
use tokio::io::{AsyncRead, AsyncWrite};

use super::flags::parse_imap_flags;
use super::session::{Io, State, no};
use crate::backend::Account;

pub(super) async fn append_flow<A, S>(
    session: &State<A>,
    tag: &str,
    framed: &mut Io<S>,
    mailbox: &str,
    flags: Option<&str>,
    literal_size: u32,
) -> Vec<String>
where
    A: Account,
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let Some(account) = session.account().cloned() else {
        return vec![format_no(tag, "not authenticated")];
    };
    let dest = match account.mailbox(mailbox).await {
        Ok(Some(d)) => d,
        Ok(None) => return vec![format_no(tag, "no such mailbox")],
        Err(e) => return vec![no(tag, "APPEND", &e)],
    };
    framed.codec_mut().expect_literal(literal_size);
    if framed
        .send(b"+ Ready for literal data\r\n".to_vec())
        .await
        .is_err()
    {
        return vec![format_no(tag, "network error")];
    }
    let bytes = match framed.next().await {
        Some(Ok(ImapInput::LiteralData(bytes))) => bytes,
        _ => return vec![format_no(tag, "expected literal data")],
    };
    let flags = flags.map(parse_imap_flags).unwrap_or_default();
    if let Err(e) = account.append(&dest, bytes.to_vec(), flags).await {
        tracing::warn!(err = %e, "imap: APPEND failed");
        return vec![format_no(tag, &format!("append failed: {e}"))];
    }
    vec![format_ok(tag, "APPEND completed")]
}
