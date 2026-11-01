use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

use super::*;

fn limits() -> Limits {
    Limits {
        max_message: 1024,
        max_command_bytes: 64 * 1024,
        command_timeout: Duration::from_secs(5),
    }
}

/// A server that answers each command line with `reply(tag, line)`.
fn serve(
    stream: DuplexStream,
    greeting: &'static str,
    reply: impl Fn(&str, &str) -> Vec<u8> + Send + 'static,
) {
    tokio::spawn(async move {
        let (r, mut w) = tokio::io::split(stream);
        let mut r = BufReader::new(r);
        w.write_all(greeting.as_bytes()).await.unwrap();
        let mut line = String::new();
        while r.read_line(&mut line).await.unwrap_or(0) > 0 {
            let l = line.trim_end().to_string();
            line.clear();
            let (tag, rest) = l.split_once(' ').unwrap_or((&l, ""));
            let out = reply(tag, rest);
            if w.write_all(&out).await.is_err() {
                return;
            }
        }
    });
}

#[tokio::test]
async fn a_session_lists_examines_and_fetches() {
    let (a, b) = tokio::io::duplex(1 << 16);
    serve(b, "* OK ready\r\n", |tag, cmd| {
        let upper = cmd.to_ascii_uppercase();
        let body = match upper.as_str() {
            c if c.starts_with("LOGIN") => {
                assert_eq!(cmd, "LOGIN \"me@old.test\" \"p\\\"w\"");
                String::new()
            }
            c if c.starts_with("LIST") => concat!(
                "* LIST (\\HasNoChildren) \"/\" INBOX\r\n",
                "* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent Items\"\r\n",
                // a literal name ending the line: the codec may or may not
                // have eaten the CRLF after it, and the next answer follows
                "* LIST (\\Noselect) \"/\" {6}\r\n[Gmai]\r\n",
                "* LIST (\\Trash) NIL \"Bin \\\"old\\\"\"\r\n",
            )
            .to_string(),
            c if c.starts_with("EXAMINE") => "* 3 EXISTS\r\n".to_string(),
            c if c.starts_with("UID FETCH 2:*") => concat!(
                "* 1 FETCH (UID 2 RFC822.SIZE 10 FLAGS (\\Seen \\Answered))\r\n",
                "* 2 FETCH (UID 5 RFC822.SIZE 99 FLAGS ())\r\n",
            )
            .to_string(),
            c if c.starts_with("UID FETCH 5 ") => {
                "* 2 FETCH (UID 5 BODY[] {12}\r\nSubject: x\r\n FLAGS ())\r\n".to_string()
            }
            _ => String::new(),
        };
        format!("{body}{tag} OK done\r\n").into_bytes()
    });
    let mut c = Client::open(a, limits(), false).await.unwrap();
    c.login("me@old.test", "p\"w").await.unwrap();
    let folders = c.list().await.unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["INBOX", "Sent Items", "[Gmai]", "Bin \"old\""]);
    assert_eq!(folders[1].flags, ["\\HasNoChildren", "\\Sent"]);
    c.examine("INBOX").await.unwrap();
    let listed = c.messages_from(2).await.unwrap();
    assert_eq!(
        listed,
        [
            Listed {
                uid: 2,
                size: 10,
                seen: true
            },
            Listed {
                uid: 5,
                size: 99,
                seen: false
            },
        ]
    );
    assert_eq!(c.fetch(5).await.unwrap(), b"Subject: x\r\n");
}

/// A literal larger than a message may be is refused on its declared size,
/// before any of it is read.
#[tokio::test]
async fn an_oversized_literal_is_refused_before_it_is_read() {
    let (a, b) = tokio::io::duplex(1 << 16);
    serve(b, "* OK ready\r\n", |_, _| {
        b"* 1 FETCH (UID 1 BODY[] {60000000}\r\n".to_vec()
    });
    let mut c = Client::open(a, limits(), false).await.unwrap();
    match c.fetch(1).await {
        Err(Error::TooLarge(60_000_000)) => {}
        other => panic!("{other:?}"),
    }
}

/// A server that answers forever fails the command at the byte budget.
#[tokio::test]
async fn endless_output_fails_the_command() {
    let (a, b) = tokio::io::duplex(1 << 16);
    serve(b, "* OK ready\r\n", |_, _| {
        "* OK more\r\n".repeat(20_000).into_bytes()
    });
    let mut c = Client::open(a, limits(), false).await.unwrap();
    match c.list().await {
        Err(Error::Overrun) => {}
        other => panic!("{other:?}"),
    }
}

/// A server that stops answering fails the command at the deadline.
#[tokio::test]
async fn silence_fails_the_command() {
    let (a, b) = tokio::io::duplex(1 << 16);
    serve(b, "* OK ready\r\n", |_, _| Vec::new());
    let mut short = limits();
    short.command_timeout = Duration::from_millis(200);
    let mut c = Client::open(a, short, false).await.unwrap();
    match c.list().await {
        Err(Error::Timeout) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_refused_login_says_what_the_server_said() {
    let (a, b) = tokio::io::duplex(1 << 16);
    serve(b, "* OK ready\r\n", |tag, _| {
        format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\x07\r\n").into_bytes()
    });
    let mut c = Client::open(a, limits(), false).await.unwrap();
    let e = c.login("me", "wrong").await.unwrap_err();
    assert_eq!(
        e.to_string(),
        "the server refused: NO [AUTHENTICATIONFAILED] Invalid credentials"
    );
}

#[tokio::test]
async fn a_greeting_that_is_not_ok_is_refused() {
    let (a, b) = tokio::io::duplex(1 << 16);
    serve(b, "* BYE too busy\r\n", |_, _| Vec::new());
    assert!(matches!(
        Client::open(a, limits(), false).await,
        Err(Error::Refused(_))
    ));
}

#[test]
fn shown_text_is_short_and_plain() {
    let long = format!("<b>x</b>\r\n{}", "y".repeat(500));
    let s = shown(&long);
    assert_eq!(s.chars().count(), 200);
    assert!(!s.contains('\r'));
}
