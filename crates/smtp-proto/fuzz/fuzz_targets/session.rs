#![no_main]
//! Fuzz the full Session state machine. Splits input on `\r\n` and feeds
//! each line as a command. Goal: any sequence of commands produces a
//! well-formed Event sequence without panic.

use libfuzzer_sys::fuzz_target;
use mailrs_smtp_proto::{Session, SessionConfig, parse_command};

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let mut sess = Session::new("mail.example.com", SessionConfig::default());
        for line in text.split("\r\n").take(64) {
            if let Ok(cmd) = parse_command(line) {
                let _ = sess.handle_command(&cmd);
            }
        }
    }
});
