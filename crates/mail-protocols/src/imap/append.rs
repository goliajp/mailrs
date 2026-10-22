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
    let limit = framed.codec().max_literal();
    if literal_size > limit {
        // refused before the client is asked to send it
        return vec![format_no(
            tag,
            &format!("[TOOBIG] messages are limited to {limit} bytes"),
        )];
    }
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
    let flags = flags
        .map(flag_list)
        .map(parse_imap_flags)
        .unwrap_or_default();
    match account.append(&dest, bytes.to_vec(), flags).await {
        Ok(filed) => vec![format_ok(
            tag,
            &format!(
                "[APPENDUID {} {}] APPEND completed",
                filed.uidvalidity, filed.uid
            ),
        )],
        Err(e) => {
            tracing::warn!(err = %e, "imap: APPEND failed");
            vec![format_no(tag, &format!("append failed: {e}"))]
        }
    }
}

/// The flag list of what the parser leaves between the mailbox and the
/// literal, `(\Seen) "22-Oct-2026 10:00:00 +0000"`: the date is not a flag.
fn flag_list(between: &str) -> &str {
    let between = between.trim_start();
    match between.strip_prefix('(') {
        Some(rest) => rest.split_once(')').map_or(rest, |(list, _)| list),
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::flag_list;

    #[test]
    fn the_date_after_the_flags_is_not_read_as_flags() {
        assert_eq!(
            flag_list("(\\Seen \\Flagged) \"22-Oct-2026 10:00:00 +0000\""),
            "\\Seen \\Flagged"
        );
        assert_eq!(flag_list("(\\Seen)"), "\\Seen");
        assert_eq!(flag_list("\"22-Oct-2026 10:00:00 +0000\""), "");
    }
}
