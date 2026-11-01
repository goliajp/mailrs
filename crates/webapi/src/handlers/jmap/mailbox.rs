//! Mailboxes: the places a conversation can be (inbox, archive, sent,
//! junk, trash, drafts), which are roles and cannot be changed, and the
//! user's labels, one mailbox each, which `Mailbox/set` makes, renames and
//! deletes. Being in a place belongs to the conversation here, so every
//! message of a conversation is in the same places; a message the user
//! wrote is in Sent as well.

use mailrs_core_api::types::ConversationFilter;
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use super::ctx::{Ctx, fail, requested_ids};
use super::view::{Counts, View};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Inbox,
    Archive,
    Sent,
    Drafts,
    Junk,
    Trash,
}

pub const ROLES: [Role; 6] = [
    Role::Inbox,
    Role::Archive,
    Role::Sent,
    Role::Drafts,
    Role::Junk,
    Role::Trash,
];

impl Role {
    pub fn id(self) -> &'static str {
        match self {
            Role::Inbox => "inbox",
            Role::Archive => "archive",
            Role::Sent => "sent",
            Role::Drafts => "drafts",
            Role::Junk => "junk",
            Role::Trash => "trash",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Role::Inbox => "Inbox",
            Role::Archive => "Archive",
            Role::Sent => "Sent",
            Role::Drafts => "Drafts",
            Role::Junk => "Junk",
            Role::Trash => "Trash",
        }
    }

    pub fn parse(id: &str) -> Option<Role> {
        ROLES.into_iter().find(|r| r.id() == id)
    }
}

/// A mailbox id: a role, or `L` + a label id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Role(Role),
    Label(String),
}

impl Place {
    pub fn id(&self) -> String {
        match self {
            Place::Role(r) => r.id().to_string(),
            Place::Label(l) => format!("L{l}"),
        }
    }

    pub fn parse(id: &str) -> Option<Place> {
        match Role::parse(id) {
            Some(r) => Some(Place::Role(r)),
            None => id
                .strip_prefix('L')
                .filter(|l| !l.is_empty())
                .map(|l| Place::Label(l.to_string())),
        }
    }

    /// The conversation list holding this mailbox's messages. Drafts are
    /// filed with what the user sent; which of those are drafts is the
    /// view's to say.
    pub fn filter(&self, limit: u32) -> ConversationFilter {
        let base = ConversationFilter {
            limit,
            ..Default::default()
        };
        match self {
            Place::Role(Role::Inbox) => ConversationFilter {
                folder: Some("nonjunk".into()),
                ..base
            },
            Place::Role(Role::Archive) => ConversationFilter {
                archived: true,
                ..base
            },
            Place::Role(Role::Sent | Role::Drafts) => ConversationFilter {
                folder: Some("Sent".into()),
                ..base
            },
            Place::Role(Role::Junk) => ConversationFilter {
                folder: Some("Junk".into()),
                ..base
            },
            Place::Role(Role::Trash) => ConversationFilter {
                trashed: true,
                ..base
            },
            Place::Label(l) => ConversationFilter {
                label: Some(l.clone()),
                ..base
            },
        }
    }
}

/// Every mailbox of the caller: the roles, then the labels by name.
pub async fn all(ctx: &Ctx) -> Result<Vec<(Place, String, Option<Role>)>, JmapMethodError> {
    let mut out: Vec<(Place, String, Option<Role>)> = ROLES
        .into_iter()
        .map(|r| (Place::Role(r), r.name().to_string(), Some(r)))
        .collect();
    let labels = ctx.state.core.list_labels(&ctx.user).await.map_err(fail)?;
    out.extend(
        labels
            .into_iter()
            .map(|l| (Place::Label(l.id), l.name, None)),
    );
    Ok(out)
}

fn object(place: &Place, name: &str, role: Option<Role>, order: usize, c: Counts) -> Value {
    let is_label = role.is_none();
    json!({
        "id": place.id(),
        "name": name,
        "parentId": null,
        "role": role.map(Role::id),
        "sortOrder": order,
        "totalEmails": c.total_emails,
        "unreadEmails": c.unread_emails,
        "totalThreads": c.total_threads,
        "unreadThreads": c.unread_threads,
        "isSubscribed": true,
        "myRights": {
            "mayReadItems": true,
            "mayAddItems": true,
            "mayRemoveItems": true,
            "maySetSeen": true,
            "maySetKeywords": true,
            "mayCreateChild": false,
            "mayRename": is_label,
            "mayDelete": is_label,
            "maySubmit": true,
        },
    })
}

/// Every mailbox with its object, in order.
pub async fn objects(ctx: &Ctx, view: &View) -> Result<Vec<(String, Value)>, JmapMethodError> {
    let counts = view.counts();
    Ok(all(ctx)
        .await?
        .into_iter()
        .enumerate()
        .map(|(n, (place, name, role))| {
            let id = place.id();
            let c = counts.get(&id).copied().unwrap_or_default();
            (id, object(&place, &name, role, n * 10, c))
        })
        .collect())
}

/// `objects` is every mailbox, as [`objects`] made them for this request.
pub fn get(
    ctx: &Ctx,
    objects: &[(String, Value)],
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let wanted = requested_ids(args);
    let mut list = Vec::new();
    let mut found = Vec::new();
    for (id, object) in objects {
        if wanted.as_ref().is_some_and(|w| !w.contains(id)) {
            continue;
        }
        found.push(id.clone());
        list.push(object.clone());
    }
    let not_found: Vec<String> = wanted
        .unwrap_or_default()
        .into_iter()
        .filter(|w| !found.contains(w))
        .collect();
    Ok((
        "Mailbox/get".into(),
        json!({ "accountId": ctx.user, "state": state, "list": list, "notFound": not_found }),
    ))
}

pub async fn query(
    ctx: &Ctx,
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let role = args
        .get("filter")
        .and_then(|f| f.get("role"))
        .and_then(|v| v.as_str());
    let ids: Vec<String> = all(ctx)
        .await?
        .into_iter()
        .filter(|(_, _, r)| role.is_none_or(|want| r.is_some_and(|r| r.id() == want)))
        .map(|(p, _, _)| p.id())
        .collect();
    Ok((
        "Mailbox/query".into(),
        json!({
            "accountId": ctx.user,
            "queryState": state,
            "canCalculateChanges": false,
            "position": 0,
            "total": ids.len(),
            "ids": ids,
        }),
    ))
}
