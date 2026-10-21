//! PUT preconditions, shared by events and contacts.

use crate::error::DavError;

/// `If-Match` (the current etag must equal the one given) and
/// `If-None-Match: *` (the resource must not exist), against the resource's
/// current etag. Shared by event and contact PUT.
pub(crate) fn check(
    current: Option<&str>,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> Result<(), DavError> {
    if let Some(expected) = if_match {
        let expected = expected.trim().trim_matches('"');
        if current != Some(expected) {
            return Err(DavError::PreconditionFailed);
        }
    }
    if let Some(inm) = if_none_match
        && inm.trim() == "*"
        && current.is_some()
    {
        return Err(DavError::PreconditionFailed);
    }
    Ok(())
}
