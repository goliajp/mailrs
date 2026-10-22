//! FETCH: the items of each addressed message, with the CONDSTORE
//! `CHANGEDSINCE` modifier and the `MODSEQ` item (RFC 7162).

use mailrs_imap_proto::{format_fetch, format_no, format_ok};

use super::fetch_items::{answer, needs_bytes, requested};
use super::query::expand_sequence;
use super::session::{State, no};
use crate::backend::Account;

pub(super) async fn fetch_response<A: Account>(
    session: &State<A>,
    tag: &str,
    sequence: &str,
    attributes: &str,
    by_uid: bool,
) -> Vec<String> {
    let State::Selected {
        account, messages, ..
    } = session
    else {
        return vec![format_no(tag, "not in SELECTED state")];
    };
    let ids = expand_sequence(sequence, messages, by_uid);
    // CHANGEDSINCE modifier (RFC 7162): `FETCH 1:* (FLAGS) (CHANGEDSINCE 42)`
    let attrs_upper = attributes.to_uppercase();
    let changedsince = attrs_upper.find("CHANGEDSINCE").and_then(|pos| {
        attributes
            .get(pos + "CHANGEDSINCE".len()..)
            .unwrap_or("")
            .split_whitespace()
            .next()
            .and_then(|t| t.trim_end_matches(')').parse::<u64>().ok())
    });
    let mut out = Vec::with_capacity(ids.len() + 1);
    for msg in ids {
        if let Some(since) = changedsince
            && msg.modseq <= since
        {
            continue;
        }
        let wanted = requested(attributes, by_uid);
        let bytes = match needs_bytes(&wanted) {
            true => match account.read(&msg).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    out.push(no(tag, "FETCH", &e));
                    return out;
                }
            },
            false => None,
        };
        let mut items = answer(&msg, wanted, bytes.as_deref());
        // MODSEQ item: explicit request or implied by CHANGEDSINCE
        if changedsince.is_some() || attrs_upper.contains("MODSEQ") {
            items.push(("MODSEQ".into(), format!("({})", msg.modseq)));
        }
        out.push(format_fetch(msg.seqno, &items));
    }
    out.push(format_ok(tag, "FETCH completed"));
    out
}
