//! End-to-end example: parse SMTP command lines, drive a `Session`, and
//! print the wire-format response each step would produce.
//!
//! Run with: `cargo run -p mailrs-smtp-proto --example parse_and_drive`
//!
//! No network I/O — the example just shows how to glue the parser, state
//! machine, and response formatter together.

use mailrs_smtp_proto::{Command, Event, Response, Session, SessionConfig, parse_command};

fn main() {
    let mut session = Session::new("smtp.example.com", SessionConfig::default());

    // simulate an incoming connection: write the greeting first
    print!(">>> {}", Response::greeting(&session.hostname).format_greeting());

    let lines = [
        "EHLO client.example.org",
        "MAIL FROM:<alice@example.org>",
        "RCPT TO:<bob@example.com>",
        "DATA",
    ];

    for line in lines {
        println!("<<< {line}");

        let cmd = match parse_command(line) {
            Ok(cmd) => cmd,
            Err(e) => {
                print!(">>> {}", Response::syntax_error().format());
                eprintln!("    (parse error: {e:?})");
                continue;
            }
        };

        match session.handle_command(&cmd) {
            Event::Reply(resp) => {
                // EHLO needs multi-line; everything else is single-line
                if let Command::Ehlo(_) = cmd {
                    let caps = session.capabilities();
                    print!(
                        ">>> {}",
                        mailrs_smtp_proto::format_ehlo_response(&session.hostname, &caps)
                    );
                } else {
                    print!(">>> {}", resp.format());
                }
            }
            Event::NeedData {
                reverse_path,
                forward_paths,
            } => {
                print!(">>> {}", Response::data_start().format());
                println!("    [server would now read message body for");
                println!("     from={reverse_path}, to={forward_paths:?}, until <CRLF>.<CRLF>]");
                print!(">>> {}", Response::data_ok().format());
            }
            Event::Shutdown(resp) => {
                print!(">>> {}", resp.format());
                println!("    [server closes connection]");
                break;
            }
            Event::StartTls(resp) => {
                print!(">>> {}", resp.format());
                println!("    [server would upgrade to TLS here, then call session.reset_after_tls()]");
                break;
            }
            Event::NeedAuth { username, .. } => {
                println!("    [server would verify credentials for {username}]");
            }
            Event::AuthChallenge { response, .. } => {
                print!(">>> {}", response.format());
            }
        }
    }
}
