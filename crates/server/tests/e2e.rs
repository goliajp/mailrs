use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

async fn start_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _addr) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                handle_test_connection(stream).await;
            });
        }
    });

    port
}

/// minimal SMTP session handler for testing
async fn handle_test_connection(stream: TcpStream) {
    use bytes::{Buf, BytesMut};
    use futures_util::{SinkExt, StreamExt};
    use mailrs_smtp_proto::response::{format_ehlo_response, Response};
    use mailrs_smtp_proto::session::{Event, Session, SessionConfig};
    use mailrs_smtp_proto::{parse_command, unstuff_data};
    use mailrs_storage_maildir::Maildir;
    use tokio_util::codec::{Decoder, Encoder, Framed};

    // inline codec that supports data mode
    struct Codec {
        data_mode: bool,
    }
    impl Codec {
        fn new() -> Self {
            Self { data_mode: false }
        }
    }

    #[derive(Debug)]
    enum Input {
        Command(String),
        Data(Vec<u8>),
    }

    impl Decoder for Codec {
        type Item = Input;
        type Error = std::io::Error;
        fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
            if self.data_mode {
                if let Some(pos) = src
                    .windows(5)
                    .position(|w| w == b"\r\n.\r\n")
                    .map(|p| p + 2)
                {
                    let data = src.split_to(pos + 3).to_vec();
                    self.data_mode = false;
                    return Ok(Some(Input::Data(data)));
                }
                Ok(None)
            } else if let Some(pos) = src.windows(2).position(|w| w == b"\r\n") {
                let line = src.split_to(pos);
                src.advance(2);
                Ok(Some(Input::Command(
                    String::from_utf8_lossy(&line).into_owned(),
                )))
            } else {
                Ok(None)
            }
        }
    }

    impl Encoder<String> for Codec {
        type Error = std::io::Error;
        fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
            dst.extend_from_slice(item.as_bytes());
            Ok(())
        }
    }

    let hostname = "mx.test.local";
    let maildir_root = std::env::temp_dir()
        .join(format!("mailrs-e2e-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let config = SessionConfig::default();
    let mut session = Session::new(hostname, config);
    let mut framed = Framed::new(stream, Codec::new());

    framed
        .send(Response::greeting(hostname).format_greeting())
        .await
        .ok();

    while let Some(Ok(input)) = framed.next().await {
        match input {
            Input::Command(line) => match parse_command(&line) {
                Ok(cmd) => {
                    if matches!(
                        cmd,
                        mailrs_smtp_proto::Command::Ehlo(_) | mailrs_smtp_proto::Command::Helo(_)
                    ) {
                        let event = session.handle_command(&cmd);
                        if matches!(event, Event::Reply(ref r) if r.code == 250) {
                            let caps = session.capabilities();
                            let resp = format_ehlo_response(hostname, &caps);
                            framed.send(resp).await.ok();
                            continue;
                        }
                    }

                    let event = session.handle_command(&cmd);
                    match event {
                        Event::Reply(resp) => {
                            framed.send(resp.format()).await.ok();
                        }
                        Event::NeedData {
                            reverse_path: _,
                            forward_paths,
                        } => {
                            framed.send(Response::data_start().format()).await.ok();
                            framed.codec_mut().data_mode = true;

                            if let Some(Ok(Input::Data(raw))) = framed.next().await {
                                let body = unstuff_data(&raw);
                                let mut ok = true;
                                for rcpt in &forward_paths {
                                    if let Some((local, domain)) = rcpt.split_once('@') {
                                        let path = format!("{maildir_root}/{domain}/{local}");
                                        match Maildir::create(&path) {
                                            Ok(md) => {
                                                if md.deliver(&body).is_err() {
                                                    ok = false;
                                                }
                                            }
                                            Err(_) => ok = false,
                                        }
                                    }
                                }
                                let resp = if ok {
                                    Response::data_ok()
                                } else {
                                    Response::new(451, None, "error")
                                };
                                framed.send(resp.format()).await.ok();
                            } else {
                                break;
                            }
                        }
                        Event::Shutdown(resp) => {
                            framed.send(resp.format()).await.ok();
                            break;
                        }
                        _ => {
                            framed.send(Response::bad_sequence().format()).await.ok();
                        }
                    }
                }
                Err(_) => {
                    framed.send(Response::syntax_error().format()).await.ok();
                }
            },
            Input::Data(_) => break,
        }
    }
}

// ---- test helpers ----

async fn connect(
    port: u16,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (read, write) = stream.into_split();
    (BufReader::new(read), write)
}

async fn read_line(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    line
}

async fn read_multiline(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> String {
    let mut result = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let cont = line.len() >= 4 && line.as_bytes()[3] == b'-';
        result.push_str(&line);
        if !cont {
            break;
        }
    }
    result
}

async fn send(writer: &mut tokio::net::tcp::OwnedWriteHalf, cmd: &str) {
    writer
        .write_all(format!("{cmd}\r\n").as_bytes())
        .await
        .unwrap();
}

/// start an SMTP test server that allows AUTH without TLS
async fn start_auth_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _addr) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                handle_auth_test_connection(stream).await;
            });
        }
    });

    port
}

/// SMTP session handler that supports AUTH PLAIN/LOGIN without requiring TLS
async fn handle_auth_test_connection(stream: TcpStream) {
    use bytes::{Buf, BytesMut};
    use futures_util::{SinkExt, StreamExt};
    use mailrs_smtp_proto::response::{format_ehlo_response, Response};
    use mailrs_smtp_proto::session::{AuthStep, Event, Session, SessionConfig};
    use mailrs_smtp_proto::{parse_command, unstuff_data, Command};
    use mailrs_storage_maildir::Maildir;
    use tokio_util::codec::{Decoder, Encoder, Framed};

    struct Codec {
        data_mode: bool,
    }
    impl Codec {
        fn new() -> Self {
            Self { data_mode: false }
        }
    }

    #[derive(Debug)]
    enum Input {
        Command(String),
        Data(Vec<u8>),
    }

    impl Decoder for Codec {
        type Item = Input;
        type Error = std::io::Error;
        fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
            if self.data_mode {
                if let Some(pos) = src
                    .windows(5)
                    .position(|w| w == b"\r\n.\r\n")
                    .map(|p| p + 2)
                {
                    let data = src.split_to(pos + 3).to_vec();
                    self.data_mode = false;
                    return Ok(Some(Input::Data(data)));
                }
                Ok(None)
            } else if let Some(pos) = src.windows(2).position(|w| w == b"\r\n") {
                let line = src.split_to(pos);
                src.advance(2);
                Ok(Some(Input::Command(
                    String::from_utf8_lossy(&line).into_owned(),
                )))
            } else {
                Ok(None)
            }
        }
    }

    impl Encoder<String> for Codec {
        type Error = std::io::Error;
        fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
            dst.extend_from_slice(item.as_bytes());
            Ok(())
        }
    }

    let hostname = "mx.test.local";
    let maildir_root = std::env::temp_dir()
        .join(format!("mailrs-e2e-auth-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    let config = SessionConfig {
        tls_available: false,
        tls_active: false,
        require_tls_for_auth: false,
        max_size: 52428800,
        max_recipients: 100,
    };
    let mut session = Session::new(hostname, config);
    let mut framed = Framed::new(stream, Codec::new());
    let mut pending_auth_step: Option<AuthStep> = None;

    // test credentials: user "testuser", password "testpass"
    let valid_user = "testuser";
    let valid_pass = "testpass";

    framed
        .send(Response::greeting(hostname).format_greeting())
        .await
        .ok();

    while let Some(Ok(input)) = framed.next().await {
        match input {
            Input::Command(line) => {
                // handle AUTH continuation responses
                if let Some(step) = pending_auth_step.take() {
                    let event = session.handle_auth_response(&line, &step);
                    match event {
                        Event::NeedAuth { username, password } => {
                            if username == valid_user && password == valid_pass {
                                session.set_authenticated(username);
                                framed
                                    .send(Response::auth_ok().format())
                                    .await
                                    .ok();
                            } else {
                                framed
                                    .send(Response::auth_failed().format())
                                    .await
                                    .ok();
                            }
                        }
                        Event::AuthChallenge { response, step: next_step } => {
                            framed.send(response.format()).await.ok();
                            pending_auth_step = Some(next_step);
                        }
                        Event::Reply(resp) => {
                            framed.send(resp.format()).await.ok();
                        }
                        _ => {
                            framed.send(Response::bad_sequence().format()).await.ok();
                        }
                    }
                    continue;
                }

                match parse_command(&line) {
                    Ok(cmd) => {
                        if matches!(
                            cmd,
                            Command::Ehlo(_) | Command::Helo(_)
                        ) {
                            let event = session.handle_command(&cmd);
                            if matches!(event, Event::Reply(ref r) if r.code == 250) {
                                let caps = session.capabilities();
                                let resp = format_ehlo_response(hostname, &caps);
                                framed.send(resp).await.ok();
                                continue;
                            }
                        }

                        let event = session.handle_command(&cmd);
                        match event {
                            Event::Reply(resp) => {
                                framed.send(resp.format()).await.ok();
                            }
                            Event::NeedAuth { username, password } => {
                                if username == valid_user && password == valid_pass {
                                    session.set_authenticated(username);
                                    framed
                                        .send(Response::auth_ok().format())
                                        .await
                                        .ok();
                                } else {
                                    framed
                                        .send(Response::auth_failed().format())
                                        .await
                                        .ok();
                                }
                            }
                            Event::AuthChallenge { response, step } => {
                                framed.send(response.format()).await.ok();
                                pending_auth_step = Some(step);
                            }
                            Event::NeedData {
                                reverse_path: _,
                                forward_paths,
                            } => {
                                framed.send(Response::data_start().format()).await.ok();
                                framed.codec_mut().data_mode = true;

                                if let Some(Ok(Input::Data(raw))) = framed.next().await {
                                    let body = unstuff_data(&raw);
                                    let mut ok = true;
                                    for rcpt in &forward_paths {
                                        if let Some((local, domain)) = rcpt.split_once('@') {
                                            let path =
                                                format!("{maildir_root}/{domain}/{local}");
                                            match Maildir::create(&path) {
                                                Ok(md) => {
                                                    if md.deliver(&body).is_err() {
                                                        ok = false;
                                                    }
                                                }
                                                Err(_) => ok = false,
                                            }
                                        }
                                    }
                                    let resp = if ok {
                                        Response::data_ok()
                                    } else {
                                        Response::new(451, None, "error")
                                    };
                                    framed.send(resp.format()).await.ok();
                                } else {
                                    break;
                                }
                            }
                            Event::Shutdown(resp) => {
                                framed.send(resp.format()).await.ok();
                                break;
                            }
                            _ => {
                                framed.send(Response::bad_sequence().format()).await.ok();
                            }
                        }
                    }
                    Err(_) => {
                        framed.send(Response::syntax_error().format()).await.ok();
                    }
                }
            }
            Input::Data(_) => break,
        }
    }
}

/// start a minimal IMAP test server
async fn start_imap_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _addr) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                handle_imap_test_connection(stream).await;
            });
        }
    });

    port
}

/// minimal IMAP session handler for testing greeting and basic commands
async fn handle_imap_test_connection(stream: TcpStream) {
    use mailrs_imap_proto::{
        format_bad, format_bye, format_capability, format_ok, ImapCommand,
    };

    let hostname = "imap.test.local";
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    // send IMAP greeting
    let greeting = format!("* OK [{hostname}] IMAP4rev1 server ready\r\n");
    write.write_all(greeting.as_bytes()).await.ok();

    let capabilities = ["IMAP4rev1", "AUTH=PLAIN", "IDLE", "QUOTA"];
    let mut authenticated = false;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            _ => {}
        }
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        match mailrs_imap_proto::parse_command(line) {
            Ok(tagged) => {
                let tag = &tagged.tag;
                match &tagged.command {
                    ImapCommand::Capability => {
                        let cap_resp = format_capability(&capabilities);
                        write.write_all(cap_resp.as_bytes()).await.ok();
                        let ok = format_ok(tag, "CAPABILITY completed");
                        write.write_all(ok.as_bytes()).await.ok();
                    }
                    ImapCommand::Login { username, password } => {
                        if username == "testuser" && password == "testpass" {
                            authenticated = true;
                            let ok = format_ok(tag, "LOGIN completed");
                            write.write_all(ok.as_bytes()).await.ok();
                        } else {
                            let no =
                                mailrs_imap_proto::format_no(tag, "LOGIN failed");
                            write.write_all(no.as_bytes()).await.ok();
                        }
                    }
                    ImapCommand::Noop => {
                        let ok = format_ok(tag, "NOOP completed");
                        write.write_all(ok.as_bytes()).await.ok();
                    }
                    ImapCommand::Logout => {
                        let bye = format_bye("server logging out");
                        write.write_all(bye.as_bytes()).await.ok();
                        let ok = format_ok(tag, "LOGOUT completed");
                        write.write_all(ok.as_bytes()).await.ok();
                        break;
                    }
                    ImapCommand::List { .. } => {
                        if !authenticated {
                            let no = mailrs_imap_proto::format_no(
                                tag,
                                "LIST requires authentication",
                            );
                            write.write_all(no.as_bytes()).await.ok();
                        } else {
                            let list_line = mailrs_imap_proto::format_list(
                                "\\HasNoChildren",
                                "/",
                                "INBOX",
                            );
                            write.write_all(list_line.as_bytes()).await.ok();
                            let ok = format_ok(tag, "LIST completed");
                            write.write_all(ok.as_bytes()).await.ok();
                        }
                    }
                    _ => {
                        if !authenticated {
                            let no = mailrs_imap_proto::format_no(
                                tag,
                                "command requires authentication",
                            );
                            write.write_all(no.as_bytes()).await.ok();
                        } else {
                            let bad = format_bad(tag, "command not supported in test");
                            write.write_all(bad.as_bytes()).await.ok();
                        }
                    }
                }
            }
            Err(_) => {
                // cannot parse tag, send untagged BAD
                write
                    .write_all(b"* BAD invalid command\r\n")
                    .await
                    .ok();
            }
        }
    }
}

// ---- tests ----

// ==== SMTP greeting tests ====

#[tokio::test]
async fn e2e_greeting_contains_esmtp() {
    let port = start_server().await;
    let (mut reader, _writer) = connect(port).await;

    let greeting = read_line(&mut reader).await;
    assert!(
        greeting.starts_with("220 "),
        "greeting must start with 220: {greeting}"
    );
    assert!(
        greeting.contains("ESMTP"),
        "greeting should contain ESMTP identifier: {greeting}"
    );
    assert!(
        greeting.contains("mx.test.local"),
        "greeting should contain server hostname: {greeting}"
    );
}

// ==== SMTP EHLO extension tests ====

#[tokio::test]
async fn e2e_ehlo_advertises_pipelining() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("PIPELINING"),
        "EHLO must advertise PIPELINING: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_advertises_size() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("SIZE"),
        "EHLO must advertise SIZE extension: {ehlo}"
    );
    // default max_size is 52428800
    assert!(
        ehlo.contains("SIZE 52428800"),
        "EHLO SIZE should show default max size: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_multiline_format() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;

    // first line should be "250-hostname"
    let lines: Vec<&str> = ehlo.lines().collect();
    assert!(
        lines.len() >= 2,
        "EHLO response should have multiple lines: {ehlo}"
    );
    assert!(
        lines[0].starts_with("250-"),
        "first EHLO line must use continuation: {ehlo}"
    );
    assert!(
        lines[0].contains("mx.test.local"),
        "first EHLO line must contain hostname: {ehlo}"
    );
    // last line should use "250 " (space, not dash)
    let last = lines.last().unwrap();
    assert!(
        last.starts_with("250 "),
        "last EHLO line must use space separator: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_no_auth_without_tls() {
    // default config requires TLS for auth, so AUTH should not be advertised
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        !ehlo.contains("AUTH"),
        "AUTH should not be advertised without TLS: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_auth_when_tls_not_required() {
    // auth server has require_tls_for_auth = false
    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("AUTH PLAIN LOGIN"),
        "AUTH PLAIN LOGIN should be advertised when TLS not required: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP HELO fallback ====

#[tokio::test]
async fn e2e_helo_basic() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "HELO old.client").await;
    let resp = read_multiline(&mut reader).await;
    assert!(
        resp.contains("250"),
        "HELO should return 250: {resp}"
    );

    // should still be able to send mail after HELO
    send(&mut writer, "MAIL FROM:<sender@example.com>").await;
    let mail_resp = read_line(&mut reader).await;
    assert!(
        mail_resp.starts_with("250 "),
        "MAIL FROM after HELO should work: {mail_resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP MAIL FROM / RCPT TO / DATA flow tests ====

#[tokio::test]
async fn e2e_rcpt_to_without_mail_from() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // RCPT TO without MAIL FROM should fail
    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("503 "),
        "RCPT TO without MAIL FROM should return 503: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_data_without_rcpt_to() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    // DATA without RCPT TO should fail
    send(&mut writer, "DATA").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("503 "),
        "DATA without RCPT TO should return 503: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_multiple_rcpt_to() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<sender@example.com>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    // multiple recipients
    send(&mut writer, "RCPT TO:<alice@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "first RCPT TO should succeed: {resp}");

    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "second RCPT TO should succeed: {resp}");

    send(&mut writer, "RCPT TO:<carol@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "third RCPT TO should succeed: {resp}");

    send(&mut writer, "DATA").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("354 "));

    writer
        .write_all(b"Subject: Multi-rcpt\r\n\r\nHello all\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "DATA should succeed with multiple rcpts: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_null_reverse_path() {
    // bounce messages use MAIL FROM:<> (null sender)
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "null reverse path should be accepted: {resp}"
    );

    send(&mut writer, "RCPT TO:<postmaster>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "RCPT TO postmaster should work: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_two_transactions_one_connection() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // first transaction
    send(&mut writer, "MAIL FROM:<first@example.com>").await;
    read_line(&mut reader).await;
    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    read_line(&mut reader).await;
    send(&mut writer, "DATA").await;
    read_line(&mut reader).await;
    writer
        .write_all(b"Subject: First\r\n\r\nFirst message\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "first transaction should succeed: {resp}");

    // second transaction without re-EHLO
    send(&mut writer, "MAIL FROM:<second@example.com>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "second MAIL FROM should succeed after completed transaction: {resp}"
    );

    send(&mut writer, "RCPT TO:<alice@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "DATA").await;
    read_line(&mut reader).await;
    writer
        .write_all(b"Subject: Second\r\n\r\nSecond message\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "second transaction should succeed: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP global commands ====

#[tokio::test]
async fn e2e_noop_any_state() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // NOOP before EHLO
    send(&mut writer, "NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "NOOP in Connected state: {resp}");

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // NOOP after EHLO
    send(&mut writer, "NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "NOOP in Greeted state: {resp}");

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    // NOOP in MailFrom state
    send(&mut writer, "NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "NOOP in MailFrom state: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_vrfy_returns_252() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "VRFY user@example.com").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("252 "),
        "VRFY should return 252: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_help_returns_214() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "HELP").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("214 "),
        "HELP should return 214: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_unknown_command_returns_500() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "XYZZY").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("500 "),
        "unknown command should return 500: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP AUTH tests (no TLS required) ====

#[tokio::test]
async fn e2e_auth_plain_inline_success() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(ehlo.contains("AUTH PLAIN LOGIN"));

    // AUTH PLAIN with inline credentials: base64(\0testuser\0testpass)
    let creds = base64::engine::general_purpose::STANDARD
        .encode(b"\x00testuser\x00testpass");
    send(&mut writer, &format!("AUTH PLAIN {creds}")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("235 "),
        "AUTH PLAIN with valid creds should return 235: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_plain_inline_failure() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // AUTH PLAIN with wrong password
    let creds = base64::engine::general_purpose::STANDARD
        .encode(b"\x00testuser\x00wrongpass");
    send(&mut writer, &format!("AUTH PLAIN {creds}")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("535 "),
        "AUTH PLAIN with wrong creds should return 535: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_plain_two_step() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // AUTH PLAIN without initial response -> 334 challenge
    send(&mut writer, "AUTH PLAIN").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("334 "),
        "AUTH PLAIN without creds should return 334 challenge: {resp}"
    );

    // send credentials in response
    let creds = base64::engine::general_purpose::STANDARD
        .encode(b"\x00testuser\x00testpass");
    send(&mut writer, &creds).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("235 "),
        "credentials should be accepted: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_login_success() {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // AUTH LOGIN -> 334 username challenge
    send(&mut writer, "AUTH LOGIN").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("334 "),
        "AUTH LOGIN should return 334 username prompt: {resp}"
    );
    // challenge should be base64("Username:")
    assert!(
        resp.contains("VXNlcm5hbWU6"),
        "challenge should contain base64 of 'Username:': {resp}"
    );

    // send username
    send(&mut writer, &b64.encode(b"testuser")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("334 "),
        "should get 334 password prompt: {resp}"
    );
    // challenge should be base64("Password:")
    assert!(
        resp.contains("UGFzc3dvcmQ6"),
        "challenge should contain base64 of 'Password:': {resp}"
    );

    // send password
    send(&mut writer, &b64.encode(b"testpass")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("235 "),
        "AUTH LOGIN should succeed with 235: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_login_failure() {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "AUTH LOGIN").await;
    read_line(&mut reader).await; // 334 username

    send(&mut writer, &b64.encode(b"testuser")).await;
    read_line(&mut reader).await; // 334 password

    send(&mut writer, &b64.encode(b"badpassword")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("535 "),
        "AUTH LOGIN with wrong password should return 535: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_requires_tls_by_default() {
    // default smtp server requires TLS for auth
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // trying AUTH without TLS on default server should get 530
    send(&mut writer, "AUTH PLAIN dGVzdA==").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("530 "),
        "AUTH without TLS should return 530: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_authenticated_mail_flow() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // authenticate first
    let creds = base64::engine::general_purpose::STANDARD
        .encode(b"\x00testuser\x00testpass");
    send(&mut writer, &format!("AUTH PLAIN {creds}")).await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("235 "));

    // send mail after authentication
    send(&mut writer, "MAIL FROM:<testuser@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "MAIL FROM after auth should work: {resp}"
    );

    send(&mut writer, "RCPT TO:<recipient@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "DATA").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("354 "));

    writer
        .write_all(b"Subject: Auth test\r\n\r\nAuthenticated message\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "DATA after auth should succeed: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== IMAP tests ====

#[tokio::test]
async fn e2e_imap_greeting_format() {
    let port = start_imap_server().await;
    let (mut reader, _writer) = connect(port).await;

    let greeting = read_line(&mut reader).await;
    assert!(
        greeting.starts_with("* OK"),
        "IMAP greeting must start with '* OK': {greeting}"
    );
    assert!(
        greeting.contains("IMAP4rev1"),
        "IMAP greeting must contain IMAP4rev1: {greeting}"
    );
    assert!(
        greeting.contains("imap.test.local"),
        "IMAP greeting must contain hostname: {greeting}"
    );
    assert!(
        greeting.ends_with("\r\n"),
        "IMAP greeting must end with CRLF"
    );
}

#[tokio::test]
async fn e2e_imap_capability() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await; // greeting

    send(&mut writer, "a001 CAPABILITY").await;
    // read capability response (untagged) + OK
    let cap_line = read_line(&mut reader).await;
    assert!(
        cap_line.starts_with("* CAPABILITY"),
        "CAPABILITY response should be untagged: {cap_line}"
    );
    assert!(
        cap_line.contains("IMAP4rev1"),
        "CAPABILITY must include IMAP4rev1: {cap_line}"
    );
    assert!(
        cap_line.contains("AUTH=PLAIN"),
        "CAPABILITY must include AUTH=PLAIN: {cap_line}"
    );
    assert!(
        cap_line.contains("IDLE"),
        "CAPABILITY must include IDLE: {cap_line}"
    );

    let ok_line = read_line(&mut reader).await;
    assert!(
        ok_line.starts_with("a001 OK"),
        "tagged OK response expected: {ok_line}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await; // BYE
    read_line(&mut reader).await; // OK
}

#[tokio::test]
async fn e2e_imap_login_success() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 LOGIN testuser testpass").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 OK"),
        "LOGIN with valid creds should return OK: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_login_failure() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 LOGIN testuser wrongpass").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 NO"),
        "LOGIN with wrong creds should return NO: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_logout() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 LOGOUT").await;
    let bye = read_line(&mut reader).await;
    assert!(
        bye.starts_with("* BYE"),
        "LOGOUT should produce BYE: {bye}"
    );
    let ok = read_line(&mut reader).await;
    assert!(
        ok.starts_with("a001 OK"),
        "LOGOUT should produce tagged OK: {ok}"
    );
}

#[tokio::test]
async fn e2e_imap_noop() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 OK"),
        "NOOP should return OK: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_list_requires_auth() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // LIST without authentication
    send(&mut writer, "a001 LIST \"\" \"*\"").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 NO"),
        "LIST without auth should return NO: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_list_after_auth() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // login first
    send(&mut writer, "a001 LOGIN testuser testpass").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("a001 OK"));

    // LIST after auth
    send(&mut writer, "a002 LIST \"\" \"*\"").await;
    let list_line = read_line(&mut reader).await;
    assert!(
        list_line.starts_with("* LIST"),
        "LIST should return untagged LIST response: {list_line}"
    );
    assert!(
        list_line.contains("INBOX"),
        "LIST should contain INBOX: {list_line}"
    );

    let ok = read_line(&mut reader).await;
    assert!(ok.starts_with("a002 OK"), "LIST should end with OK: {ok}");

    send(&mut writer, "a003 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

// ==== SMTP connection behavior tests ====

#[tokio::test]
async fn e2e_quit_before_ehlo() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // QUIT without EHLO should still work
    send(&mut writer, "QUIT").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("221 "),
        "QUIT before EHLO should return 221: {resp}"
    );
}

#[tokio::test]
async fn e2e_ehlo_resets_session() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO first.test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    send(&mut writer, "RCPT TO:<x@test.local>").await;
    read_line(&mut reader).await;

    // second EHLO should reset the session
    send(&mut writer, "EHLO second.test").await;
    read_multiline(&mut reader).await;

    // RCPT TO should fail because transaction was reset
    send(&mut writer, "RCPT TO:<y@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("503 "),
        "RCPT TO after EHLO reset should return 503: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_rset_in_connected_state() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // RSET before EHLO should return 250
    send(&mut writer, "RSET").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "RSET in Connected state should return 250: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_rapid_connect_disconnect() {
    let port = start_server().await;

    // rapidly connect and disconnect multiple times
    for _ in 0..5 {
        let (mut reader, mut writer) = connect(port).await;
        let greeting = read_line(&mut reader).await;
        assert!(greeting.starts_with("220 "));

        send(&mut writer, "QUIT").await;
        let resp = read_line(&mut reader).await;
        assert!(resp.starts_with("221 "));
    }
}

// ==== original tests ====

#[tokio::test]
async fn e2e_basic_session() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    // greeting
    let greeting = read_line(&mut reader).await;
    assert!(
        greeting.starts_with("220 "),
        "expected 220 greeting: {greeting}"
    );

    // EHLO
    send(&mut writer, "EHLO test.client").await;
    let ehlo_resp = read_multiline(&mut reader).await;
    assert!(
        ehlo_resp.contains("250"),
        "expected 250 EHLO response: {ehlo_resp}"
    );

    // MAIL FROM
    send(&mut writer, "MAIL FROM:<alice@example.com>").await;
    let mail_resp = read_line(&mut reader).await;
    assert!(mail_resp.starts_with("250 "), "expected 250: {mail_resp}");

    // RCPT TO
    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    let rcpt_resp = read_line(&mut reader).await;
    assert!(rcpt_resp.starts_with("250 "), "expected 250: {rcpt_resp}");

    // DATA
    send(&mut writer, "DATA").await;
    let data_resp = read_line(&mut reader).await;
    assert!(data_resp.starts_with("354 "), "expected 354: {data_resp}");

    // message body
    writer
        .write_all(b"Subject: Test\r\n\r\nHello world\r\n.\r\n")
        .await
        .unwrap();
    let queued = read_line(&mut reader).await;
    assert!(queued.starts_with("250 "), "expected 250 queued: {queued}");

    // QUIT
    send(&mut writer, "QUIT").await;
    let quit_resp = read_line(&mut reader).await;
    assert!(quit_resp.starts_with("221 "), "expected 221: {quit_resp}");
}

#[tokio::test]
async fn e2e_bad_sequence() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    read_line(&mut reader).await; // greeting

    // MAIL FROM without EHLO
    send(&mut writer, "MAIL FROM:<alice@example.com>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("503 "), "expected 503: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_multiple_sessions() {
    let port = start_server().await;

    let handles: Vec<_> = (0..3)
        .map(|i| {
            tokio::spawn(async move {
                let (mut reader, mut writer) = connect(port).await;
                read_line(&mut reader).await; // greeting

                send(&mut writer, &format!("EHLO client{i}.test")).await;
                read_multiline(&mut reader).await;

                send(&mut writer, &format!("MAIL FROM:<user{i}@example.com>")).await;
                let resp = read_line(&mut reader).await;
                assert!(resp.starts_with("250 "), "session {i} MAIL failed: {resp}");

                send(&mut writer, &format!("RCPT TO:<rcpt{i}@test.local>")).await;
                let resp = read_line(&mut reader).await;
                assert!(resp.starts_with("250 "), "session {i} RCPT failed: {resp}");

                send(&mut writer, "DATA").await;
                read_line(&mut reader).await;

                writer
                    .write_all(format!("Subject: Test {i}\r\n\r\nBody {i}\r\n.\r\n").as_bytes())
                    .await
                    .unwrap();
                let resp = read_line(&mut reader).await;
                assert!(resp.starts_with("250 "), "session {i} DATA failed: {resp}");

                send(&mut writer, "QUIT").await;
                read_line(&mut reader).await;
            })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn e2e_pipelining() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    read_line(&mut reader).await; // greeting

    send(&mut writer, "EHLO pipeline.test").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("PIPELINING"),
        "server should advertise PIPELINING"
    );

    send(&mut writer, "MAIL FROM:<a@b>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "RCPT TO:<x@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_rset_mid_transaction() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    read_line(&mut reader).await; // greeting

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    send(&mut writer, "RCPT TO:<x@test.local>").await;
    read_line(&mut reader).await;

    // RSET should reset to Greeted
    send(&mut writer, "RSET").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "expected 250 for RSET: {resp}");

    // should be able to start new transaction
    send(&mut writer, "MAIL FROM:<new@sender>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "MAIL after RSET should work: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}
