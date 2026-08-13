//! Turning a sent message back into a draft, and the address helpers
//! that reconstruct its recipients.

//! The Send list — one row per send, with delivery status
//! (RFC 20260730-send-status S3).
//!
//! Distinct from the Sent conversation axis this will replace. That axis
//! lists conversations, and status is a property of an attempt: three
//! sends in one thread can be delivered, failed and retrying at once, and
//! a conversation row has nowhere to put that, nor anywhere to hang
//! "re-edit this one" when only one of the three failed.
//!
//! Nothing in the UI reads this yet. `:shadow` answers whether it is safe
//! to.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

use super::*;
use crate::handlers::kevy_util::with_kevy;
use mailrs_core_sidestate::families::send_read;

/// A failed send as compose fields, ready to reopen in the composer.
#[derive(Debug, serde::Serialize)]
pub struct RedraftResponse {
    pub redraft_of: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub html_body: String,
    pub in_reply_to: Option<String>,
    /// The carried attachments, described but not transferred. `index` is
    /// what a later send passes back in `redraft_keep`.
    pub attachments: Vec<RedraftAttachment>,
}

#[derive(Debug, serde::Serialize)]
pub struct RedraftAttachment {
    pub index: usize,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
}

/// `GET /api/mail/sends/{send_id}:redraft` — a failed send as compose
/// fields, so the user can fix what went wrong and send again.
///
/// The attachment bytes stay here. Only their names and sizes go to the
/// browser; the send that follows names the ones to keep by index and the
/// server re-extracts them. Re-editing a 15 MB mail therefore costs no
/// transfer, and cannot lose the files — which is what would happen if
/// re-edit went through the drafts table, since a draft has no
/// attachments field at all.
///
/// The metadata comes from the same `attachments_from_envelope` walk the
/// send path uses. That is the point: an index means the same part on
/// both sides because one function decides what the parts are.
pub async fn send_redraft(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Path(send_id): axum::extract::Path<String>,
) -> Result<Json<RedraftResponse>, StatusCode> {
    let user_c = user.clone();
    let send_c = send_id.clone();
    let item = with_kevy(move |c| send_read::read_one(c, &user_c, &send_c))?
        .ok_or(StatusCode::NOT_FOUND)?;
    let bytes = envelope_bytes(&user, &send_id).await?;

    let (text, html, _) = crate::handlers::conversations::parse_body(&bytes);
    let to = split_csv(&item.to_csv);
    let cc = split_csv(&item.cc_csv);
    let all: Vec<String> = item
        .recipients
        .iter()
        .map(|r| r.recipient.clone())
        .collect();
    let bcc = bcc_from(&all, &to, &cc);

    let attachments = crate::handlers::prefs::attachments_from_envelope(&bytes)
        .into_iter()
        .enumerate()
        .map(|(index, a)| RedraftAttachment {
            index,
            filename: a.filename,
            content_type: a.content_type,
            size: a.bytes.len(),
        })
        .collect();

    Ok(Json(RedraftResponse {
        redraft_of: send_id,
        to,
        cc,
        bcc,
        subject: item.subject,
        body: text.unwrap_or_default(),
        html_body: html.unwrap_or_default(),
        // Staying in the thread it was addressed to. A repair of a failed
        // send belongs where the original was going, not in a new
        // conversation.
        in_reply_to: Some(item.thread_id).filter(|t| !t.is_empty()),
        attachments,
    }))
}

/// The Bcc set: every queued recipient that is not in To or Cc.
///
/// A Bcc header is not in the envelope — it would not be blind — so this
/// is the only way to put a blind recipient back in the right field on
/// re-edit. Getting it wrong moves someone from Bcc into a visible
/// header, which is a disclosure, not a display bug.
///
/// Matching is on the bare address, lowercased. Not `contains`: `a@b.com`
/// is a substring of `xa@b.com`, and the codebase already carries one live
/// bug of exactly that shape in `senders_csv_contains_user`.
///
/// Both sides today store the header form — a captured prod row holds
/// `GOLIA <goliaaccess@gmail.com>` in `to_csv` and in the recipient list
/// alike, because both come from the same compose field. Comparing whole
/// strings would work on that data and break the moment one side is
/// normalised to a bare address, and it would break by classifying every
/// recipient as Bcc. Matching the address itself is indifferent to which
/// form each side happens to carry.
fn bcc_from(recipients: &[String], to: &[String], cc: &[String]) -> Vec<String> {
    let addressed: std::collections::HashSet<String> =
        to.iter().chain(cc.iter()).map(|s| addr_key(s)).collect();
    recipients
        .iter()
        .filter(|r| !addressed.contains(&addr_key(r)))
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect()
}

/// The comparable identity of an address: what is inside the angle
/// brackets when there are any, the whole trimmed string otherwise,
/// lowercased.
fn addr_key(raw: &str) -> String {
    let s = raw.trim();
    match (s.rfind('<'), s.rfind('>')) {
        (Some(open), Some(close)) if close > open + 1 => s[open + 1..close].trim().to_lowercase(),
        _ => s.to_lowercase(),
    }
}

/// Split a stored `to_csv` / `cc_csv` back into addresses.
fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod redraft_tests {
    use super::{bcc_from, split_csv};

    /// Bcc is reconstructed, not stored, so an error here moves a blind
    /// recipient into a visible header on the next send. That is a
    /// disclosure.
    #[test]
    fn a_blind_recipient_stays_blind_when_re_edited() {
        let to = vec!["visible@x.com".to_string()];
        let cc = vec!["copied@x.com".to_string()];
        let all = vec![
            "visible@x.com".to_string(),
            "copied@x.com".to_string(),
            "blind@x.com".to_string(),
        ];
        assert_eq!(bcc_from(&all, &to, &cc), vec!["blind@x.com".to_string()]);
    }

    /// The recipients list is what the queue was handed; To and Cc come
    /// from headers, which carry whatever case the sender typed.
    #[test]
    fn case_does_not_turn_a_visible_recipient_into_a_bcc() {
        let to = vec!["Visible@X.com".to_string()];
        let all = vec!["visible@x.com".to_string()];
        assert!(
            bcc_from(&all, &to, &[]).is_empty(),
            "the same address in different case is the same recipient"
        );
    }

    /// `a@b.com` is a substring of `xa@b.com`. Matching on containment
    /// rather than equality would drop a real Bcc recipient, and the
    /// codebase already carries one live bug of that shape
    /// (`senders_csv_contains_user`).
    #[test]
    fn a_longer_address_that_ends_with_a_visible_one_is_still_a_bcc() {
        let to = vec!["a@b.com".to_string()];
        let all = vec!["a@b.com".to_string(), "xa@b.com".to_string()];
        assert_eq!(bcc_from(&all, &to, &[]), vec!["xa@b.com".to_string()]);
    }

    /// The shape prod actually stores, captured from `GET /api/mail/sends`
    /// on 2.18.14 (2026-07-30): both `to_csv` and the recipient list hold
    /// `GOLIA <goliaaccess@gmail.com>`, display name included.
    #[test]
    fn the_header_form_prod_stores_is_matched_not_reported_as_bcc() {
        let to = vec!["GOLIA <goliaaccess@gmail.com>".to_string()];
        let all = vec!["GOLIA <goliaaccess@gmail.com>".to_string()];
        assert!(bcc_from(&all, &to, &[]).is_empty());
    }

    /// The drift this guards against: one side normalised to a bare
    /// address, the other still carrying the display name. Comparing whole
    /// strings would call this a Bcc and expose the recipient.
    #[test]
    fn a_display_name_on_one_side_only_is_still_the_same_recipient() {
        let to = vec!["GOLIA <goliaaccess@gmail.com>".to_string()];
        let all = vec!["goliaaccess@gmail.com".to_string()];
        assert!(
            bcc_from(&all, &to, &[]).is_empty(),
            "the same address is the same recipient whichever form it is written in"
        );
    }

    /// A genuine Bcc alongside a display-name To must survive the
    /// normalisation above — the point is to match forms, not to match
    /// everything.
    #[test]
    fn normalising_forms_does_not_swallow_a_real_bcc() {
        let to = vec!["GOLIA <goliaaccess@gmail.com>".to_string()];
        let all = vec![
            "goliaaccess@gmail.com".to_string(),
            "Blind One <blind@x.com>".to_string(),
        ];
        assert_eq!(
            bcc_from(&all, &to, &[]),
            vec!["Blind One <blind@x.com>".to_string()]
        );
    }

    #[test]
    fn csv_splitting_tolerates_the_spacing_a_header_carries() {
        assert_eq!(
            split_csv("a@x.com, b@x.com ,, c@x.com"),
            vec![
                "a@x.com".to_string(),
                "b@x.com".to_string(),
                "c@x.com".to_string()
            ]
        );
        assert!(split_csv("").is_empty());
    }
}
