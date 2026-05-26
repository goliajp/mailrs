//! Minimal IMAP mock server.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// start a minimal IMAP test server
pub async fn start_imap_server() -> u16 {
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
    use mailrs_imap_proto::{ImapCommand, format_bad, format_bye, format_capability, format_ok};

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
                            let no = mailrs_imap_proto::format_no(tag, "LOGIN failed");
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
                            let no =
                                mailrs_imap_proto::format_no(tag, "LIST requires authentication");
                            write.write_all(no.as_bytes()).await.ok();
                        } else {
                            let list_line =
                                mailrs_imap_proto::format_list("\\HasNoChildren", "/", "INBOX");
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
                write.write_all(b"* BAD invalid command\r\n").await.ok();
            }
        }
    }
}
