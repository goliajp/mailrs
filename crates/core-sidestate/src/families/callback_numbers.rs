//! The phone numbers each sender has given a reader, across conversations.
//!
//! ```text
//! mailrs:callnum:{user}:{sender}   hash   digits -> "{arrival} {message_id}"
//!                                         last   -> latest arrival recorded
//! ```
//!
//! One hash per reader and sender address. A field is a number's digits;
//! its value is when the earliest message carrying it arrived (unix
//! seconds, this deployment's own `Received:` stamp) and that message's id.
//! The reading pane asks for it to point out a number the sender has not
//! given the reader before: a number is known before a message when the
//! earliest message that carried it is another one. Recording converges:
//! the same message recorded twice writes nothing the second time.
//!
//! Bounded twice: at most [`MAX_PER_SENDER`] numbers per sender, the
//! earliest kept (a sender who gave more than that, mostly bulk mail with
//! a number per shop, has its later numbers read as new), and the whole
//! hash expires [`KEEP_DAYS`] after the latest message recorded into it. The receive path (each recipient's copy, the message's text) and
//! fastcore's attachment reader (the attachments' text) write; the
//! reading pane's route reads.

use std::io;
use std::time::Duration;

use kevy_client::Connection;

const PREFIX: &str = "mailrs:callnum:";

/// The field holding the latest arrival recorded; never a number's digits.
const LAST: &str = "last";

/// Numbers kept per reader and sender.
pub const MAX_PER_SENDER: usize = 32;

/// Days a sender's numbers are kept after the last message that gave any.
pub const KEEP_DAYS: u64 = 400;

/// The hash of the numbers `sender` gave `user`. Both lowercased.
pub fn key(user: &str, sender: &str) -> String {
    format!(
        "{PREFIX}{}:{}",
        user.trim().to_ascii_lowercase(),
        sender.trim().to_ascii_lowercase()
    )
}

/// One number a sender gave, and the earliest message that gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    /// The number's digits.
    pub number: String,
    /// When that message arrived, unix seconds.
    pub first_seen: i64,
    /// That message's id.
    pub first_message_id: String,
}

/// Record that `message_id`, from `sender` to `user` and arrived at `at`,
/// gave the numbers whose digits are `numbers`. An earlier arrival already
/// recorded for a number stands. Answers how many numbers were new or
/// moved to an earlier message; 0 when nothing was written.
pub fn record(
    conn: &mut Connection,
    user: &str,
    sender: &str,
    numbers: &[String],
    at: i64,
    message_id: &str,
) -> io::Result<usize> {
    if numbers.is_empty() || user.trim().is_empty() || sender.trim().is_empty() {
        return Ok(0);
    }
    record_at(
        conn,
        &key(user, sender),
        numbers,
        at,
        message_id,
        MAX_PER_SENDER,
        KEEP_DAYS,
    )
}

/// [`record`] into the hash `k`, keeping at most `max` values, the whole
/// hash expiring `keep_days` after the latest arrival recorded. Shared with
/// the other per-sender histories built the same way.
pub(super) fn record_at(
    conn: &mut Connection,
    k: &str,
    numbers: &[String],
    at: i64,
    message_id: &str,
    max: usize,
    keep_days: u64,
) -> io::Result<usize> {
    let k = k.to_string();
    let (mut held, last) = read(conn, &k)?;
    let mut changed = 0;
    for n in numbers {
        match held.iter_mut().find(|g| &g.number == n) {
            Some(g) if g.first_seen <= at => continue,
            Some(g) => {
                g.first_seen = at;
                g.first_message_id = message_id.to_string();
            }
            None => {
                // Full: the latest number goes if this one is earlier, else
                // this one is not kept. Keeping the earliest is the same
                // whatever order mail is recorded in, so a backfill run
                // twice changes nothing the second time.
                if held.len() >= max {
                    let Some(i) = (0..held.len())
                        .max_by_key(|&i| (held[i].first_seen, held[i].number.clone()))
                    else {
                        continue;
                    };
                    if (at, n) >= (held[i].first_seen, &held[i].number) {
                        continue;
                    }
                    let gone = held.remove(i);
                    conn.hdel(k.as_bytes(), &[gone.number.as_bytes()])?;
                }
                held.push(Given {
                    number: n.clone(),
                    first_seen: at,
                    first_message_id: message_id.to_string(),
                });
            }
        }
        changed += 1;
        let value = format!("{at} {message_id}");
        conn.hset(k.as_bytes(), &[(n.as_bytes(), value.as_bytes())])?;
    }
    if last.is_none_or(|l| l < at) {
        conn.hset(
            k.as_bytes(),
            &[(LAST.as_bytes(), at.to_string().as_bytes())],
        )?;
        conn.expire(k.as_bytes(), Duration::from_secs(keep_days * 86_400))?;
    }
    Ok(changed)
}

/// [`record`] for numbers as written in a message from `from` (the decoded
/// header), by their digits.
pub fn record_numbers(
    conn: &mut Connection,
    user: &str,
    from: &str,
    numbers: &[String],
    at: i64,
    message_id: &str,
) -> io::Result<usize> {
    let sender = super::correspondent::first_address(from);
    let mut digits: Vec<String> = Vec::new();
    for n in numbers {
        let d = mailrs_fraud::callback::digits(n);
        if !digits.contains(&d) {
            digits.push(d);
        }
    }
    record(conn, user, &sender, &digits, at, message_id)
}

/// The numbers `sender` gave `user`, earliest first.
pub fn given(conn: &mut Connection, user: &str, sender: &str) -> io::Result<Vec<Given>> {
    let (mut held, _) = read(conn, &key(user, sender))?;
    held.sort_by(|a, b| (a.first_seen, &a.number).cmp(&(b.first_seen, &b.number)));
    Ok(held)
}

/// Every value in the hash `k`, and the latest arrival recorded.
pub(super) fn read(conn: &mut Connection, k: &str) -> io::Result<(Vec<Given>, Option<i64>)> {
    let flat = conn.hgetall(k.as_bytes())?;
    let mut last = None;
    let mut held = Vec::new();
    for pair in flat.chunks(2) {
        let [f, v] = pair else { continue };
        let field = String::from_utf8_lossy(f);
        let value = String::from_utf8_lossy(v);
        if field == LAST {
            last = value.trim().parse().ok();
            continue;
        }
        let (when, id) = value.split_once(' ').unwrap_or((&value, ""));
        let Ok(first_seen) = when.trim().parse() else {
            continue;
        };
        held.push(Given {
            number: field.into_owned(),
            first_seen,
            first_message_id: id.to_string(),
        });
    }
    Ok((held, last))
}

#[cfg(test)]
mod tests;
