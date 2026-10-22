//! What a connection may do before it is encrypted (RFC 9051 §6.2.1),
//! and AUTHENTICATE PLAIN (RFC 4616), which signs in like LOGIN.
//!
//! These verbs are read off the raw line before the general parser,
//! which knows neither STARTTLS nor AUTHENTICATE, and CAPABILITY is
//! answered here because it depends on whether TLS is still on offer.

use futures_util::{SinkExt, StreamExt};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use mailrs_imap_proto::{format_bad, format_capability, format_no, format_ok};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

const PRIVACY_REQUIRED: &str = "[PRIVACYREQUIRED] use STARTTLS first";

/// `starttls`: a plaintext connection with STARTTLS on offer, where no
/// login is possible until it is used.
pub(super) fn capabilities(starttls: bool) -> Vec<&'static str> {
    let mut caps = vec!["IMAP4rev1"];
    if starttls {
        caps.extend(["STARTTLS", "LOGINDISABLED"]);
    } else {
        caps.push("AUTH=PLAIN");
    }
    caps.extend([
        "IDLE",
        "NAMESPACE",
        "ENABLE",
        "UNSELECT",
        "MOVE",
        "SPECIAL-USE",
        "SORT",
        "QUOTA",
        "CONDSTORE",
        "QRESYNC",
    ]);
    caps
}

pub(super) enum Early {
    /// not one of these verbs: parse the line as usual
    Parse,
    Reply(Vec<String>),
    /// answer OK, then hand the socket to TLS
    StartTls,
    /// a decoded AUTHENTICATE PLAIN, to be run as LOGIN
    Login {
        username: String,
        password: String,
    },
    Closed,
}

pub(super) async fn early<S>(
    line: &str,
    tag: &str,
    starttls: bool,
    framed: &mut Framed<S, ImapCodec>,
) -> Early
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let rest = line.strip_prefix(tag).unwrap_or(line).trim_start();
    let (verb, args) = rest.split_once(' ').unwrap_or((rest, ""));
    match verb.to_ascii_uppercase().as_str() {
        "CAPABILITY" => Early::Reply(vec![
            format_capability(&capabilities(starttls)),
            format_ok(tag, "CAPABILITY completed"),
        ]),
        // one personal namespace, the root, with LIST's "/" delimiter
        // (RFC 2342); no other users' or shared mailboxes
        "NAMESPACE" => Early::Reply(vec![
            "* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n".to_string(),
            format_ok(tag, "NAMESPACE completed"),
        ]),
        "STARTTLS" if starttls => Early::StartTls,
        "STARTTLS" => Early::Reply(vec![format_bad(tag, "STARTTLS not available")]),
        "LOGIN" | "AUTHENTICATE" if starttls => {
            Early::Reply(vec![format_no(tag, PRIVACY_REQUIRED)])
        }
        "AUTHENTICATE" => authenticate(tag, args.trim(), framed).await,
        _ => Early::Parse,
    }
}

async fn authenticate<S>(tag: &str, args: &str, framed: &mut Framed<S, ImapCodec>) -> Early
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    let (mech, initial) = args.split_once(' ').unwrap_or((args, ""));
    if !mech.eq_ignore_ascii_case("PLAIN") {
        return Early::Reply(vec![format_no(tag, "unsupported mechanism")]);
    }
    let response = match initial.trim() {
        "" => {
            if framed.send(b"+ \r\n".to_vec()).await.is_err() {
                return Early::Closed;
            }
            match framed.next().await {
                Some(Ok(ImapInput::Line(l))) => l.trim().to_string(),
                Some(Ok(ImapInput::LiteralData(_))) => String::new(),
                _ => return Early::Closed,
            }
        }
        // SASL-IR: "=" is an empty initial response
        "=" => String::new(),
        r => r.to_string(),
    };
    if response == "*" {
        return Early::Reply(vec![format_bad(tag, "AUTHENTICATE cancelled")]);
    }
    match mailrs_smtp_proto::auth::decode_plain(&response) {
        Ok((username, password)) => Early::Login { username, password },
        Err(_) => Early::Reply(vec![format_bad(tag, "malformed PLAIN response")]),
    }
}
