//! `Email/query`: one condition (no AND/OR/NOT operators), sorted by
//! `receivedAt`, optionally one email per thread.
//!
//! Conversations come newest first, each dated by its newest message, so
//! expanding them in order and stopping once the next one is older than
//! the K-th newest message already collected gives the exact top K
//! without reading every conversation.

use mailrs_core_api::method::conversation::{ListConversationsRequest, SearchConversationsRequest};
use mailrs_core_api::method::message::MessageWire;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use super::ctx::{Ctx, fail};
use super::ids;
use super::mailbox::Place;
use super::view::View;

/// Conversations read for one query.
const SCAN_LIMIT: u32 = 5000;
/// Results per call when the client names no limit.
const DEFAULT_LIMIT: usize = 50;

const KNOWN: [&str; 13] = [
    "inMailbox",
    "text",
    "subject",
    "from",
    "to",
    "body",
    "before",
    "after",
    "hasKeyword",
    "notKeyword",
    "hasAttachment",
    "minSize",
    "maxSize",
];

fn unsupported(why: &str) -> JmapMethodError {
    JmapMethodError::Other {
        kind: "unsupportedFilter".into(),
        description: why.into(),
    }
}

fn epoch(v: Option<&Value>) -> Option<i64> {
    v.and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp())
}

struct Filter {
    place: Option<Place>,
    search: Option<String>,
    before: Option<i64>,
    after: Option<i64>,
    has: Option<u32>,
    not: Option<u32>,
    attachment: Option<bool>,
    min: Option<u64>,
    max: Option<u64>,
}

fn filter_of(v: Option<&Value>) -> Result<Filter, JmapMethodError> {
    let empty = serde_json::Map::new();
    let f = match v {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) if m.contains_key("operator") => {
            return Err(unsupported("filter operators are not supported"));
        }
        Some(Value::Object(m)) => m,
        Some(_) => return Err(JmapMethodError::InvalidArguments("filter".into())),
    };
    if let Some(k) = f.keys().find(|k| !KNOWN.contains(&k.as_str())) {
        return Err(unsupported(&format!("filter on {k} is not supported")));
    }
    let s = |k: &str| f.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let keyword = |k: &str| match s(k) {
        Some(kw) => {
            let bit = mailrs_jmap::flags::keyword_to_flag(&kw);
            match bit {
                0 => Err(unsupported(&format!("keyword {kw} is not supported"))),
                b => Ok(Some(b)),
            }
        }
        None => Ok(None),
    };
    let place = match s("inMailbox") {
        Some(id) => Some(Place::parse(&id).ok_or_else(|| unsupported("no such mailbox"))?),
        None => None,
    };
    let search = ["text", "subject", "from", "to", "body"]
        .iter()
        .filter_map(|k| s(k))
        .collect::<Vec<_>>();
    Ok(Filter {
        place,
        search: (!search.is_empty()).then(|| search.join(" ")),
        before: epoch(f.get("before")),
        after: epoch(f.get("after")),
        has: keyword("hasKeyword")?,
        not: keyword("notKeyword")?,
        attachment: f.get("hasAttachment").and_then(|v| v.as_bool()),
        min: f.get("minSize").and_then(|v| v.as_u64()),
        max: f.get("maxSize").and_then(|v| v.as_u64()),
    })
}

impl Filter {
    /// Keywords and mailboxes are read from the view, the same place
    /// `Email/get` and the change record read them.
    fn admits(&self, view: &View, w: &MessageWire) -> bool {
        let Some(m) = view.get(w.uid) else {
            return false;
        };
        self.place
            .as_ref()
            .is_none_or(|p| view.places(m).contains(p))
            && self.before.is_none_or(|b| w.internal_date < b)
            && self.after.is_none_or(|a| w.internal_date >= a)
            && self.has.is_none_or(|bit| m.flags & bit != 0)
            && self.not.is_none_or(|bit| m.flags & bit == 0)
            && self
                .attachment
                .is_none_or(|want| (w.attachments.unwrap_or(0) > 0) == want)
            && self.min.is_none_or(|m| u64::from(w.size) >= m)
            && self.max.is_none_or(|m| u64::from(w.size) < m)
    }
}

/// The conversations the filter's place and search allow, newest first.
async fn conversations(
    ctx: &Ctx,
    f: &Filter,
) -> Result<Vec<ConversationSummaryWire>, JmapMethodError> {
    let core = &ctx.state.core;
    if let Some(q) = &f.search {
        let req = SearchConversationsRequest {
            query: q.clone(),
            filter: ConversationFilter {
                limit: SCAN_LIMIT,
                ..Default::default()
            },
            quarantined: false,
        };
        let mut rows = core
            .search_conversations(&ctx.user, &req)
            .await
            .map_err(fail)?
            .items;
        rows.sort_by_key(|r| std::cmp::Reverse(r.last_date));
        return Ok(rows);
    }
    let filter = match &f.place {
        Some(place) => place.filter(SCAN_LIMIT),
        None => ConversationFilter {
            folder: Some("all".into()),
            limit: SCAN_LIMIT,
            ..Default::default()
        },
    };
    let req = ListConversationsRequest { filter };
    Ok(core
        .list_conversations(&ctx.user, &req)
        .await
        .map_err(fail)?
        .items)
}

/// A parsed `Email/query`: what it selects and in which order.
pub struct Query {
    filter: Filter,
    ascending: bool,
    collapse: bool,
}

pub fn parse(args: &Value) -> Result<Query, JmapMethodError> {
    if args.get("anchor").is_some_and(|a| !a.is_null()) {
        return Err(JmapMethodError::Other {
            kind: "unsupportedFilter".into(),
            description: "anchor is not supported; use position".into(),
        });
    }
    let ascending = match args.get("sort").and_then(|v| v.as_array()) {
        None => false,
        Some(list) => match list.first() {
            None => false,
            Some(s) if s.get("property").and_then(|v| v.as_str()) == Some("receivedAt") => s
                .get("isAscending")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            Some(_) => {
                return Err(JmapMethodError::Other {
                    kind: "unsupportedSort".into(),
                    description: "only receivedAt".into(),
                });
            }
        },
    };
    Ok(Query {
        filter: filter_of(args.get("filter"))?,
        ascending,
        collapse: args.get("collapseThreads").and_then(|v| v.as_bool()) == Some(true),
    })
}

/// The uids the query selects, in its order. `needed` is how many from
/// the newest end the caller will read; `usize::MAX` reads them all.
pub async fn results(
    ctx: &Ctx,
    view: &View,
    q: &Query,
    needed: usize,
) -> Result<Vec<u32>, JmapMethodError> {
    let convs = conversations(ctx, &q.filter).await?;
    // ascending order needs every conversation; otherwise the newest
    // `needed` are enough
    let needed = match q.ascending {
        true => usize::MAX,
        false => needed,
    };
    let mut found: Vec<MessageWire> = Vec::new();
    for conv in &convs {
        if found.len() >= needed {
            let kth = found[needed - 1].internal_date;
            if conv.last_date < kth {
                break;
            }
        }
        let msgs = ctx
            .state
            .core
            .list_thread_messages(&ctx.user, &conv.thread_id)
            .await
            .map_err(fail)?
            .items;
        let mut hits: Vec<MessageWire> = msgs
            .into_iter()
            .filter(|w| q.filter.admits(view, w))
            .collect();
        if q.collapse {
            hits.sort_by_key(|w| std::cmp::Reverse(w.internal_date));
            hits.truncate(1);
        }
        found.extend(hits);
        found.sort_by_key(|w| (std::cmp::Reverse(w.internal_date), std::cmp::Reverse(w.uid)));
    }
    if q.ascending {
        found.reverse();
    }
    Ok(found.into_iter().map(|w| w.uid).collect())
}

pub async fn query(
    ctx: &Ctx,
    view: &View,
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let q = parse(args)?;
    let position = args.get("position").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map_or(DEFAULT_LIMIT, |l| (l as usize).min(super::MAX_OBJECTS));
    let want_total = args.get("calculateTotal").and_then(|v| v.as_bool()) == Some(true);
    let needed = match want_total {
        true => usize::MAX,
        false => position + limit,
    };
    let found = results(ctx, view, &q, needed).await?;
    let total = found.len();
    let ids: Vec<String> = found
        .iter()
        .skip(position)
        .take(limit)
        .map(|uid| ids::email(*uid))
        .collect();
    let mut out = json!({
        "accountId": ctx.user,
        "queryState": state,
        "canCalculateChanges": true,
        "position": position,
        "ids": ids,
        "limit": limit,
    });
    if want_total {
        out["total"] = json!(total);
    }
    Ok(("Email/query".into(), out))
}
