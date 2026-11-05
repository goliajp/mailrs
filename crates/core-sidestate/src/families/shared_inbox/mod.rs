//! Team inboxes: what the local members of a shared email group hold in
//! common about the mail the group receives. Every member keeps a copy of
//! each message in their own mailbox; these keys tie the copies to one team
//! conversation and carry what the team says and decides about it.
//!
//! ```text
//! shared:{gid}:msgids                    hash   Message-ID -> conv
//! shared:{gid}:conv-counter              counter
//! shared:{gid}:note-counter              counter
//! shared:{gid}:{conv}:copies             hash   member -> their thread id
//! shared:{gid}:{conv}:assign             list   assignment facts, oldest first
//! shared:{gid}:{conv}:notes              list   note facts, oldest first
//! shared:{gid}:{conv}:presence:{member}  string viewing | composing, 60 s
//! shared:{gid}:{conv}:draft              string the reply being written
//! shared:{gid}:{conv}:handoffs           list   draft takeover facts
//! shared:claims:{member}                 hash   Message-ID -> the reply claim it holds
//! ```
//!
//! Notes are never mail: nothing here is written into a message, a queue,
//! a search index or a webhook, so no path that sends or exports mail can
//! reach them.

use kevy_client::Connection;

pub mod assign;
pub mod drafts;
pub mod notes;
pub mod presence;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod family_names {
    #[test]
    fn a_group_purge_counts_keys_by_family() {
        let f = |k: &str| super::group_family(12, k);
        assert_eq!(f("shared:12:msgids"), "shared:{gid}:msgids");
        assert_eq!(f("shared:12:note-counter"), "shared:{gid}:note-counter");
        assert_eq!(f("shared:12:3:notes"), "shared:{gid}:{conv}:notes");
        assert_eq!(
            f("shared:12:3:presence:a@x"),
            "shared:{gid}:{conv}:presence:*"
        );
    }
}

/// One team conversation: group `gid`'s conversation number `conv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConvRef {
    pub gid: i64,
    pub conv: i64,
}

impl ConvRef {
    /// `"{gid}:{conv}"`, as a member's thread row carries it.
    pub fn tag(&self) -> String {
        format!("{}:{}", self.gid, self.conv)
    }

    /// The reverse of [`Self::tag`].
    pub fn parse(tag: &str) -> Option<Self> {
        let (gid, conv) = tag.split_once(':')?;
        Some(Self {
            gid: gid.parse().ok()?,
            conv: conv.parse().ok()?,
        })
    }

    /// The key of this conversation's `what` (`notes`, `assign`, ...).
    pub fn key(&self, what: &str) -> String {
        format!("shared:{}:{}:{what}", self.gid, self.conv)
    }
}

/// Every key of group `gid`'s working state, as a glob.
pub fn group_glob(gid: i64) -> String {
    format!("shared:{gid}:*")
}

/// Every member's held reply claims, as a glob.
pub const CLAIMS_GLOB: &str = "shared:claims:*";

/// The name the reply claims members hold for one group are counted under
/// when the group is purged.
pub const GROUP_CLAIMS: &str = "shared:claims:{user} (the group's claims)";

/// Group `gid`'s key `key` under its family's name, as a purge counts it:
/// the group id as `{gid}`, a conversation number as `{conv}`, a member as
/// `*` (`shared:{gid}:{conv}:presence:*`).
pub fn group_family(gid: i64, key: &str) -> String {
    let prefix = format!("shared:{gid}:");
    let rest = key.strip_prefix(&prefix).unwrap_or(key);
    let conv_and_what = rest
        .split_once(':')
        .filter(|(conv, _)| !conv.is_empty() && conv.bytes().all(|b| b.is_ascii_digit()));
    let Some((_, what)) = conv_and_what else {
        return format!("shared:{{gid}}:{rest}");
    };
    match what.split_once(':') {
        Some((what, _)) => format!("shared:{{gid}}:{{conv}}:{what}:*"),
        None => format!("shared:{{gid}}:{{conv}}:{what}"),
    }
}

fn msgids_key(gid: i64) -> String {
    format!("shared:{gid}:msgids")
}

pub(crate) fn note_counter_key(gid: i64) -> String {
    format!("shared:{gid}:note-counter")
}

fn bare_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_ascii_lowercase()
}

/// The team conversation a message the group received belongs to: the one
/// any message it answers or cites landed in, else a new one. The message
/// is then indexed under it, so what answers it later joins too.
pub fn conv_for_arrival(
    conn: &mut Connection,
    gid: i64,
    message_id: &str,
    in_reply_to: &str,
    references: &[String],
) -> std::io::Result<ConvRef> {
    let index = msgids_key(gid);
    let own = bare_id(message_id);
    let mut ancestors: Vec<String> = vec![own.clone(), bare_id(in_reply_to)];
    ancestors.extend(references.iter().rev().map(|r| bare_id(r)));
    for id in ancestors.iter().filter(|id| !id.is_empty()) {
        let found = conn
            .hget(index.as_bytes(), id.as_bytes())?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|v| v.parse::<i64>().ok());
        if let Some(conv) = found {
            conn.hset(
                index.as_bytes(),
                &[(own.as_bytes(), conv.to_string().as_bytes())],
            )?;
            return Ok(ConvRef { gid, conv });
        }
    }
    let conv = conn.incr(format!("shared:{gid}:conv-counter").as_bytes())?;
    conn.hset(
        index.as_bytes(),
        &[(own.as_bytes(), conv.to_string().as_bytes())],
    )?;
    Ok(ConvRef { gid, conv })
}

/// Index a message a member sent for the team (a reply from the group's
/// address) under `r`, so the answer to it joins the same conversation.
pub fn index_reply(conn: &mut Connection, r: ConvRef, message_id: &str) -> std::io::Result<()> {
    let own = bare_id(message_id);
    if own.is_empty() {
        return Ok(());
    }
    conn.hset(
        msgids_key(r.gid).as_bytes(),
        &[(own.as_bytes(), r.conv.to_string().as_bytes())],
    )?;
    Ok(())
}

/// Record that `member`'s copy of the conversation is their thread
/// `thread_id`.
pub fn record_copy(
    conn: &mut Connection,
    r: ConvRef,
    member: &str,
    thread_id: &str,
) -> std::io::Result<()> {
    conn.hset(
        r.key("copies").as_bytes(),
        &[(
            member.trim().to_ascii_lowercase().as_bytes(),
            thread_id.as_bytes(),
        )],
    )?;
    Ok(())
}

/// Every member's copy: `(member, thread id)`, sorted by member.
pub fn copies(conn: &mut Connection, r: ConvRef) -> std::io::Result<Vec<(String, String)>> {
    let flat = conn.hgetall(r.key("copies").as_bytes())?;
    let mut out: Vec<(String, String)> = flat
        .chunks_exact(2)
        .filter_map(|kv| {
            Some((
                String::from_utf8(kv[0].clone()).ok()?,
                String::from_utf8(kv[1].clone()).ok()?,
            ))
        })
        .collect();
    out.sort();
    Ok(out)
}

/// The group's current members, lowercased and sorted.
pub fn members(conn: &mut Connection, gid: i64) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = conn
        .smembers(super::email_groups::members_key(gid).as_bytes())?
        .into_iter()
        .filter_map(|m| String::from_utf8(m).ok())
        .map(|m| m.trim().to_ascii_lowercase())
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Push `payload` to each of `users`' live connections, through the same
/// change feed new mail travels on. Each user gets their own event, with
/// `user` set, because a socket is sent only the events addressed to it.
pub fn publish(
    conn: &mut Connection,
    users: &[String],
    payload: &serde_json::Value,
) -> std::io::Result<()> {
    for user in users {
        let mut event = payload.clone();
        event["user"] = serde_json::Value::String(user.clone());
        let body = serde_json::to_vec(&event).map_err(std::io::Error::other)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let key = crate::keys::events::notify_key(nanos);
        conn.set(key.as_bytes(), &body)?;
        conn.expire(key.as_bytes(), std::time::Duration::from_secs(300))?;
    }
    Ok(())
}

/// Tell the team's open pages that `by` claimed reply number `seq`, so a
/// member writing an answer of their own sees it before sending.
pub fn announce_reply(
    conn: &mut Connection,
    r: ConvRef,
    by: &str,
    seq: u64,
) -> std::io::Result<()> {
    let members = members(conn, r.gid)?;
    publish(
        conn,
        &members,
        &serde_json::json!({
            "type": "SharedReplied",
            "gid": r.gid,
            "conv": r.conv,
            "by": by,
            "seq": seq,
        }),
    )
}

/// A reply claim held by a sent message, so undoing the send can release
/// it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeldClaim {
    pub gid: i64,
    pub conv: i64,
    pub claim_id: String,
}

pub fn claims_key(member: &str) -> String {
    format!("shared:claims:{}", member.trim().to_ascii_lowercase())
}

/// Remember that `member`'s message `message_id` holds `claim`.
pub fn hold_claim(
    conn: &mut Connection,
    member: &str,
    message_id: &str,
    claim: &HeldClaim,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(claim).map_err(std::io::Error::other)?;
    conn.hset(
        claims_key(member).as_bytes(),
        &[(message_id.as_bytes(), body.as_slice())],
    )?;
    Ok(())
}

/// The claim `member`'s message `message_id` held, forgotten as it is read.
pub fn take_claim(
    conn: &mut Connection,
    member: &str,
    message_id: &str,
) -> std::io::Result<Option<HeldClaim>> {
    let key = claims_key(member);
    let found = conn
        .hget(key.as_bytes(), message_id.as_bytes())?
        .and_then(|v| serde_json::from_slice(&v).ok());
    if found.is_some() {
        conn.hdel(key.as_bytes(), &[message_id.as_bytes()])?;
    }
    Ok(found)
}
