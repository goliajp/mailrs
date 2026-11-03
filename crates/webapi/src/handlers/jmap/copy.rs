//! `Email/copy` (RFC 8621 §4.7, RFC 8620 §5.4): copying between two
//! accounts. Every session here has one account, its own, so there is no
//! other account to copy from: the same account is refused as the RFC
//! says, and any other is not found.

use mailrs_jmap::error::JmapMethodError;
use serde_json::Value;

use super::ctx::Ctx;

pub fn copy(ctx: &Ctx, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let from = args.get("fromAccountId").and_then(|v| v.as_str());
    match from {
        None => Err(JmapMethodError::InvalidArguments("fromAccountId".into())),
        Some(f) if f.eq_ignore_ascii_case(&ctx.user) => Err(JmapMethodError::InvalidArguments(
            "fromAccountId must differ from accountId; within one account use Email/set".into(),
        )),
        Some(_) => Err(JmapMethodError::Other {
            kind: "fromAccountNotFound".into(),
            description: String::new(),
        }),
    }
}
