//! SMTP mock server that supports AUTH PLAIN/LOGIN without requiring TLS.

use tokio::net::{TcpListener, TcpStream};

/// start an SMTP test server that allows AUTH without TLS
pub async fn start_auth_server() -> u16 {
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
    use mailrs_maildir::Maildir;
    use mailrs_smtp_proto::response::{Response, format_ehlo_response};
    use mailrs_smtp_proto::session::{AuthStep, Event, Session, SessionConfig};
    use mailrs_smtp_proto::{Command, parse_command, unstuff_data};
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
                                framed.send(Response::auth_ok().format()).await.ok();
                            } else {
                                framed.send(Response::auth_failed().format()).await.ok();
                            }
                        }
                        Event::AuthChallenge {
                            response,
                            step: next_step,
                        } => {
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
                        if matches!(cmd, Command::Ehlo(_) | Command::Helo(_)) {
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
                                    framed.send(Response::auth_ok().format()).await.ok();
                                } else {
                                    framed.send(Response::auth_failed().format()).await.ok();
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
                }
            }
            Input::Data(_) => break,
        }
    }
}
