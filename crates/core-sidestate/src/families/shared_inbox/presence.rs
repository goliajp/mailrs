//! Who on the team has a conversation open, and who is writing a reply.
//! A hint, not a lock: each entry lives 60 seconds and the client renews
//! it every 20, so a closed tab drops out on its own.

use std::time::Duration;

use kevy_client::Connection;

use super::ConvRef;

pub const TTL: Duration = Duration::from_secs(60);

/// What a member is doing in the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Viewing,
    Composing,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Viewing => "viewing",
            State::Composing => "composing",
        }
    }

    fn named(s: &[u8]) -> Option<Self> {
        match s {
            b"viewing" => Some(State::Viewing),
            b"composing" => Some(State::Composing),
            _ => None,
        }
    }
}

fn key(r: ConvRef, member: &str) -> String {
    r.key(&format!("presence:{}", member.trim().to_ascii_lowercase()))
}

/// Renew `member`'s presence. Returns whether it changed: a member who
/// appeared, or went from viewing to composing or back.
pub fn heartbeat(
    conn: &mut Connection,
    r: ConvRef,
    member: &str,
    state: State,
) -> std::io::Result<bool> {
    let k = key(r, member);
    let before = conn.get(k.as_bytes())?.as_deref().and_then(State::named);
    conn.set_with_ttl(k.as_bytes(), state.name().as_bytes(), TTL)?;
    Ok(before != Some(state))
}

/// Drop `member`'s presence now (the conversation was closed).
pub fn leave(conn: &mut Connection, r: ConvRef, member: &str) -> std::io::Result<bool> {
    Ok(conn.del(&[key(r, member).as_bytes()])? > 0)
}

/// Who of `members` is present, and doing what.
pub fn present(
    conn: &mut Connection,
    r: ConvRef,
    members: &[String],
) -> std::io::Result<Vec<(String, State)>> {
    let mut out = Vec::new();
    for m in members {
        if let Some(s) = conn
            .get(key(r, m).as_bytes())?
            .as_deref()
            .and_then(State::named)
        {
            out.push((m.clone(), s));
        }
    }
    Ok(out)
}
