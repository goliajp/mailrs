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
                        mailrs_smtp_proto::Command::Ehlo(_)
                            | mailrs_smtp_proto::Command::Helo(_)
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
                    framed
                        .send(Response::syntax_error().format())
                        .await
                        .ok();
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

// ---- tests ----

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
    assert!(
        data_resp.starts_with("354 "),
        "expected 354: {data_resp}"
    );

    // message body
    writer
        .write_all(b"Subject: Test\r\n\r\nHello world\r\n.\r\n")
        .await
        .unwrap();
    let queued = read_line(&mut reader).await;
    assert!(
        queued.starts_with("250 "),
        "expected 250 queued: {queued}"
    );

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
                assert!(
                    resp.starts_with("250 "),
                    "session {i} MAIL failed: {resp}"
                );

                send(&mut writer, &format!("RCPT TO:<rcpt{i}@test.local>")).await;
                let resp = read_line(&mut reader).await;
                assert!(
                    resp.starts_with("250 "),
                    "session {i} RCPT failed: {resp}"
                );

                send(&mut writer, "DATA").await;
                read_line(&mut reader).await;

                writer
                    .write_all(
                        format!("Subject: Test {i}\r\n\r\nBody {i}\r\n.\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
                let resp = read_line(&mut reader).await;
                assert!(
                    resp.starts_with("250 "),
                    "session {i} DATA failed: {resp}"
                );

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
