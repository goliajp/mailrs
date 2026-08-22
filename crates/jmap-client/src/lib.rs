#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use serde_json::Value;

/// Where a server's API actually lives, and which account to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Where method calls are POSTed.
    pub api_url: String,
    /// The template blob downloads are built from.
    pub download_url: String,
    /// The account id for mail.
    pub account_id: String,
}

/// What `Email/changes` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changes {
    /// The server can no longer say what changed since that state.
    ///
    /// **This is "read the mailbox again", not an error to log.** A
    /// client that keeps asking from a state the server has forgotten
    /// never sees another message, and nothing about that looks like a
    /// failure from the outside — the same shape as trusting a stale
    /// `UIDVALIDITY`.
    StartOver,
    /// What moved, and the state to ask from next time.
    Moved {
        /// Ids created since the old state.
        created: Vec<String>,
        /// Ids destroyed since the old state.
        destroyed: Vec<String>,
        /// The state to send next time.
        new_state: String,
        /// Whether this answer is a page rather than the whole story.
        ///
        /// Ignoring it stops the sync one page in, which looks exactly
        /// like nothing new arriving.
        has_more: bool,
    },
}

/// Read the session object.
///
/// `None` when it does not name a mail account — a server that speaks
/// JMAP for contacts and not for mail is a real thing, and guessing an
/// account id from it produces requests that fail one call later with
/// a message about the wrong subject.
pub fn parse_session(body: &str) -> Option<Session> {
    let v: Value = serde_json::from_str(body).ok()?;
    let account_id = v
        .get("primaryAccounts")?
        .get("urn:ietf:params:jmap:mail")?
        .as_str()?
        .to_string();
    Some(Session {
        api_url: v.get("apiUrl")?.as_str()?.to_string(),
        download_url: v.get("downloadUrl")?.as_str()?.to_string(),
        account_id,
    })
}

/// Fill in a download URL from the session's template.
///
/// The ids are percent-escaped: a blob id may hold characters a URL
/// path cannot carry, and pasting one in verbatim produces a 404 that
/// reads like the message is gone.
pub fn blob_url(session: &Session, blob_id: &str, name: &str) -> String {
    session
        .download_url
        .replace("{accountId}", &escape(&session.account_id))
        .replace("{blobId}", &escape(blob_id))
        .replace("{name}", &escape(name))
        .replace("{type}", "application%2Foctet-stream")
}

fn escape(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Read an `Email/changes` answer.
pub fn parse_changes(body: &str) -> Option<Changes> {
    let v: Value = serde_json::from_str(body).ok()?;
    let calls = v.get("methodResponses")?.as_array()?;
    for call in calls {
        let name = call.get(0)?.as_str()?;
        let args = call.get(1)?;
        if name == "error" {
            // Every other error is the caller's to handle; this one is
            // a state, not a failure.
            if args.get("type").and_then(Value::as_str) == Some("cannotCalculateChanges") {
                return Some(Changes::StartOver);
            }
            continue;
        }
        if name != "Email/changes" {
            continue;
        }
        return Some(Changes::Moved {
            created: ids(args.get("created")),
            destroyed: ids(args.get("destroyed")),
            new_state: args.get("newState")?.as_str()?.to_string(),
            has_more: args
                .get("hasMoreChanges")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    None
}

fn ids(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
