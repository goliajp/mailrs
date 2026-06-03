//! POP3 TRANSACTION-state body commands: RETR, TOP.

use crate::message_util;

use super::{Pop3Session, Pop3State};

impl Pop3Session {
    pub(super) async fn handle_retr(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction {
            ref username,
            ref messages,
        } = self.state
        else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        let Ok(num) = arg.trim().parse::<usize>() else {
            return vec!["-ERR invalid message number\r\n".into()];
        };
        if num == 0 || num > messages.len() {
            return vec!["-ERR no such message\r\n".into()];
        }
        let m = &messages[num - 1];
        if m.deleted {
            return vec!["-ERR message deleted\r\n".into()];
        }

        let raw = message_util::read_message_raw(&self.maildir_root, username, &m.maildir_id).await;
        match raw {
            Some(data) => {
                let mut resp = vec![format!("+OK {} octets\r\n", data.len())];
                // byte-stuff lines starting with '.'
                let stuffed = mailrs_smtp_client::connection::dot_stuff(&data);
                resp.push(String::from_utf8_lossy(&stuffed).into_owned());
                if !stuffed.ends_with(b"\r\n") {
                    resp.push("\r\n".into());
                }
                resp.push(".\r\n".into());
                resp
            }
            None => vec!["-ERR message not found on disk\r\n".into()],
        }
    }

    pub(super) async fn handle_top(&self, arg: &str) -> Vec<String> {
        let Pop3State::Transaction {
            ref username,
            ref messages,
        } = self.state
        else {
            return vec!["-ERR not authenticated\r\n".into()];
        };

        let parts: Vec<&str> = arg.trim().splitn(2, ' ').collect();
        if parts.len() != 2 {
            return vec!["-ERR usage: TOP msg lines\r\n".into()];
        }
        let Ok(num) = parts[0].parse::<usize>() else {
            return vec!["-ERR invalid message number\r\n".into()];
        };
        let Ok(lines) = parts[1].parse::<usize>() else {
            return vec!["-ERR invalid line count\r\n".into()];
        };
        if num == 0 || num > messages.len() {
            return vec!["-ERR no such message\r\n".into()];
        }
        let m = &messages[num - 1];
        if m.deleted {
            return vec!["-ERR message deleted\r\n".into()];
        }

        let raw = message_util::read_message_raw(&self.maildir_root, username, &m.maildir_id).await;
        match raw {
            Some(data) => {
                let text = String::from_utf8_lossy(&data);
                // split at blank line (end of headers)
                let (headers, body) = match text.find("\r\n\r\n") {
                    Some(pos) => (&text[..pos + 2], &text[pos + 4..]),
                    None => (text.as_ref(), ""),
                };

                let mut resp = vec!["+OK\r\n".into()];
                resp.push(format!("{}\r\n", headers));
                resp.push("\r\n".into());

                // add requested number of body lines
                for (i, line) in body.lines().enumerate() {
                    if i >= lines {
                        break;
                    }
                    if line.starts_with('.') {
                        resp.push(format!(".{line}\r\n"));
                    } else {
                        resp.push(format!("{line}\r\n"));
                    }
                }
                resp.push(".\r\n".into());
                resp
            }
            None => vec!["-ERR message not found on disk\r\n".into()],
        }
    }
}
