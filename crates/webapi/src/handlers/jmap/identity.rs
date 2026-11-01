//! `Identity/get`: the addresses the account may send as — its own and
//! those it was granted. Masked addresses are not listed; there can be a
//! thousand of them, and the masked-email API is where they are managed.

use base64::Engine as _;
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use super::ctx::{Ctx, fail, requested_ids};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// `I` + the address, base64url.
pub fn id(address: &str) -> String {
    format!("I{}", B64.encode(address.to_ascii_lowercase()))
}

/// The account's own address, then each it may send as.
async fn addresses(ctx: &Ctx) -> Result<Vec<String>, JmapMethodError> {
    let mut out = vec![ctx.user.to_ascii_lowercase()];
    let granted = ctx.state.core.get_send_as(&ctx.user).await.map_err(fail)?;
    for a in granted.send_as {
        let a = a.trim().to_ascii_lowercase();
        if !a.is_empty() && !out.contains(&a) {
            out.push(a);
        }
    }
    Ok(out)
}

/// The address an identity id names, if it is one of the account's.
pub async fn address_of(ctx: &Ctx, identity_id: &str) -> Option<String> {
    addresses(ctx)
        .await
        .ok()?
        .into_iter()
        .find(|a| id(a) == identity_id)
}

pub async fn get(ctx: &Ctx, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let wanted = requested_ids(args);
    let all = addresses(ctx).await?;
    let list: Vec<Value> = all
        .iter()
        .filter(|a| wanted.as_ref().is_none_or(|w| w.contains(&id(a))))
        .map(|a| {
            json!({
                "id": id(a),
                "name": "",
                "email": a,
                "replyTo": null,
                "bcc": null,
                "textSignature": "",
                "htmlSignature": "",
                "mayDelete": false,
            })
        })
        .collect();
    let not_found: Vec<String> = wanted
        .unwrap_or_default()
        .into_iter()
        .filter(|w| !all.iter().any(|a| id(a) == *w))
        .collect();
    let state = format!(
        "{:016x}",
        mailrs_core_api::method::message_index::fnv64(all.join(",").as_bytes())
    );
    Ok((
        "Identity/get".into(),
        json!({ "accountId": ctx.user, "state": state, "list": list, "notFound": not_found }),
    ))
}
