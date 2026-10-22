//! IDLE (RFC 2177).

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::ImapInput;
use mailrs_imap_proto::{format_exists, format_no, format_ok};
use tokio::io::{AsyncRead, AsyncWrite};

use super::session::{Io, State, no};
use super::store::refresh;
use crate::backend::{Account, Watch};

/// RFC 2177 IDLE. On a change to the selected mailbox it is listed
/// again and `* n EXISTS` sent when the count grew. Ends when the client
/// sends DONE, the connection drops, or the 29-minute inactivity ceiling
/// passes (clients re-issue IDLE well before that per the RFC). With no
/// mailbox selected it reports nothing.
pub(super) async fn idle_flow<A, S>(
    session: &mut State<A>,
    tag: &str,
    framed: &mut Io<S>,
) -> Vec<String>
where
    A: Account,
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let mut watched = match &*session {
        State::NotAuthed => return vec![format_no(tag, "not authenticated")],
        State::Authed { .. } => None,
        State::Selected {
            account,
            mailbox,
            messages,
            ..
        } => Some((account.clone(), mailbox.clone(), messages.len())),
    };
    let mut watch = watched.as_ref().map(|(a, mb, _)| a.watch(mb));
    if framed.send(b"+ idling\r\n".to_vec()).await.is_err() {
        return Vec::new();
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(29 * 60);
    loop {
        tokio::select! {
            frame = framed.next() => match frame {
                Some(Ok(ImapInput::Line(l))) if l.trim().eq_ignore_ascii_case("DONE") => break,
                Some(Ok(_)) => continue,
                _ => return Vec::new(),
            },
            () = changed(watch.as_mut()) => {
                let Some((account, mailbox, known)) = watched.as_mut() else { continue };
                let fresh = match account.messages(mailbox).await {
                    Ok(m) => m,
                    Err(e) => return vec![no(tag, "IDLE", &e)],
                };
                if fresh.len() > *known {
                    *known = fresh.len();
                    if framed.send(format_exists(*known as u32).into_bytes()).await.is_err() {
                        return Vec::new();
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    // post-IDLE FETCHes see the new mail
    if let Err(e) = refresh(session).await {
        return vec![no(tag, "IDLE", &e)];
    }
    vec![format_ok(tag, "IDLE terminated")]
}

async fn changed<W: Watch>(watch: Option<&mut W>) {
    match watch {
        Some(w) => w.changed().await,
        None => std::future::pending().await,
    }
}
