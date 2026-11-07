//! Ids made by one call and named by a later one in the same request.

use reqwest::Method;
use serde_json::json;

use super::{ALICE, identity, jmap, live};

/// A client that writes a draft and sends it in one request names the
/// draft by its creation id (RFC 8620 §5.3), as RFC 8621's own example of
/// `EmailSubmission/set` does.
pub(super) async fn draft_and_send_in_one_request(live: &live::Live, alice: &str) {
    let ident = identity(live, alice).await;
    let (_, before) = live.call(Method::GET, "/api/mail/sends", alice, None).await;
    let before = before.as_array().map_or(0, |l| l.len());
    let r = jmap(
        live,
        alice,
        json!([
            ["Email/set", { "create": { "k1490": {
                "mailboxIds": { "drafts": true }, "keywords": { "$draft": true },
                "from": [{ "email": ALICE }], "to": [{ "email": "carol@partner.example" }],
                "subject": "#k1490",
                "textBody": [{ "partId": "1", "type": "text/plain" }],
                "bodyValues": { "1": { "value": "written and sent together" } }
            } } }, "0"],
            ["EmailSubmission/set", { "create": { "s": {
                "emailId": "#k1490", "identityId": ident
            } } }, "1"],
            ["Email/get", { "ids": ["#k1490"], "properties": ["subject"] }, "2"]
        ]),
    )
    .await;
    let calls = &r["methodResponses"];
    let email = calls[0][1]["created"]["k1490"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{r}"));
    assert_eq!(calls[1][0], "EmailSubmission/set", "{r}");
    assert!(calls[1][1]["created"]["s"]["id"].is_string(), "{r}");
    // the id is rewritten where an id goes, the subject is left as written
    assert_eq!(calls[2][1]["list"][0]["id"], email, "{r}");
    assert_eq!(calls[2][1]["list"][0]["subject"], "#k1490", "{r}");
    let (_, after) = live.call(Method::GET, "/api/mail/sends", alice, None).await;
    assert_eq!(
        after.as_array().map_or(0, |l| l.len()),
        before + 1,
        "{after}"
    );
}
