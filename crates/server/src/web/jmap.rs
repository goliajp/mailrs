use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use mailrs_mailbox::types::{FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_SEEN};

use super::{AuthUser, WebState};

const JMAP_CORE_CAP: &str = "urn:ietf:params:jmap:core";
const JMAP_MAIL_CAP: &str = "urn:ietf:params:jmap:mail";
const JMAP_SUBMISSION_CAP: &str = "urn:ietf:params:jmap:submission";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JmapRequest {
    #[serde(default)]
    #[allow(dead_code)]
    using: Vec<String>,
    method_calls: Vec<(String, Value, String)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JmapResponse {
    method_responses: Vec<(String, Value, String)>,
    session_state: String,
}

pub(super) async fn jmap_session(
    AuthUser { address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let api_url = format!("https://{}/jmap", state.hostname);
    let download_url = format!(
        "https://{}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}",
        state.hostname
    );
    let upload_url = format!("https://{}/jmap/upload/{{accountId}}/", state.hostname);
    let event_source_url = format!(
        "https://{}/jmap/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}",
        state.hostname
    );

    Json(serde_json::json!({
        "capabilities": {
            JMAP_CORE_CAP: {
                "maxSizeUpload": 50_000_000_u64,
                "maxConcurrentUpload": 4,
                "maxSizeRequest": 10_000_000_u64,
                "maxConcurrentRequests": 4,
                "maxCallsInRequest": 16,
                "maxObjectsInGet": 500,
                "maxObjectsInSet": 500,
                "collationAlgorithms": []
            },
            JMAP_MAIL_CAP: {
                "maxMailboxesPerEmail": null,
                "maxMailboxDepth": null,
                "maxSizeMailboxName": 255,
                "maxSizeAttachmentsPerEmail": 50_000_000_u64,
                "emailQuerySortOptions": ["receivedAt", "sentAt"],
                "mayCreateTopLevelMailbox": true
            },
            JMAP_SUBMISSION_CAP: {}
        },
        "accounts": {
            &address: {
                "name": &address,
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    JMAP_CORE_CAP: {},
                    JMAP_MAIL_CAP: {},
                    JMAP_SUBMISSION_CAP: {}
                }
            }
        },
        "primaryAccounts": {
            JMAP_CORE_CAP: &address,
            JMAP_MAIL_CAP: &address,
            JMAP_SUBMISSION_CAP: &address
        },
        "username": &address,
        "apiUrl": api_url,
        "downloadUrl": download_url,
        "uploadUrl": upload_url,
        "eventSourceUrl": event_source_url,
        "state": "0"
    }))
}

pub(super) async fn jmap_api(
    auth_user: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(request): Json<JmapRequest>,
) -> impl IntoResponse {
    let mut responses: Vec<(String, Value, String)> = Vec::new();

    for (method, mut args, call_id) in request.method_calls {
        resolve_references(&mut args, &responses);

        let result = dispatch_method(
            &method,
            &args,
            &auth_user,
            &state,
        )
        .await;

        match result {
            Ok((name, value)) => responses.push((name, value, call_id)),
            Err(err) => responses.push(("error".to_string(), err, call_id)),
        }
    }

    Json(JmapResponse {
        method_responses: responses,
        session_state: "0".to_string(),
    })
}

fn resolve_references(args: &mut Value, previous: &[(String, Value, String)]) {
    let obj = match args.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    let ref_keys: Vec<String> = obj
        .keys()
        .filter(|k| k.starts_with('#'))
        .cloned()
        .collect();

    for ref_key in ref_keys {
        let ref_val = match obj.remove(&ref_key) {
            Some(v) => v,
            None => continue,
        };

        let result_of = ref_val.get("resultOf").and_then(|v| v.as_str());
        let name = ref_val.get("name").and_then(|v| v.as_str());
        let path = ref_val.get("path").and_then(|v| v.as_str());

        let (result_of, name, path) = match (result_of, name, path) {
            (Some(r), Some(n), Some(p)) => (r, n, p),
            _ => continue,
        };

        let resolved = previous.iter().find(|(resp_name, _, resp_id)| {
            resp_id == result_of && resp_name == name
        });

        if let Some((_, resp_value, _)) = resolved {
            if let Some(val) = json_pointer(resp_value, path) {
                let real_key = ref_key.trim_start_matches('#').to_string();
                obj.insert(real_key, val.clone());
            }
        }
    }
}

fn json_pointer<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer == "/" || pointer.is_empty() {
        return Some(value);
    }
    value.pointer(pointer)
}

async fn dispatch_method(
    method: &str,
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    match method {
        "Mailbox/get" => handle_mailbox_get(args, auth, state).await,
        "Mailbox/query" => handle_mailbox_query(args, auth, state).await,
        "Email/get" => handle_email_get(args, auth, state).await,
        "Email/query" => handle_email_query(args, auth, state).await,
        "Email/set" => handle_email_set(args, auth, state).await,
        "Thread/get" => handle_thread_get(args, auth, state).await,
        "EmailSubmission/set" => handle_email_submission_set(args, auth, state).await,
        _ => Err(serde_json::json!({
            "type": "urn:ietf:params:jmap:error:unknownMethod"
        })),
    }
}

fn mailbox_role(name: &str) -> Option<&'static str> {
    match name {
        "INBOX" => Some("inbox"),
        "Sent" => Some("sent"),
        "Drafts" => Some("drafts"),
        "Trash" => Some("trash"),
        "Junk" => Some("junk"),
        _ => None,
    }
}

fn flags_to_keywords(flags: u32) -> Value {
    let mut kw = serde_json::Map::new();
    if flags & FLAG_SEEN != 0 {
        kw.insert("$seen".to_string(), Value::Bool(true));
    }
    if flags & FLAG_ANSWERED != 0 {
        kw.insert("$answered".to_string(), Value::Bool(true));
    }
    if flags & FLAG_FLAGGED != 0 {
        kw.insert("$flagged".to_string(), Value::Bool(true));
    }
    if flags & FLAG_DRAFT != 0 {
        kw.insert("$draft".to_string(), Value::Bool(true));
    }
    Value::Object(kw)
}

fn keywords_to_flags(keywords: &Value) -> u32 {
    let obj = match keywords.as_object() {
        Some(o) => o,
        None => return 0,
    };
    let mut flags = 0u32;
    for (k, v) in obj {
        if v.as_bool() != Some(true) {
            continue;
        }
        match k.as_str() {
            "$seen" => flags |= FLAG_SEEN,
            "$answered" => flags |= FLAG_ANSWERED,
            "$flagged" => flags |= FLAG_FLAGGED,
            "$draft" => flags |= FLAG_DRAFT,
            _ => {}
        }
    }
    flags
}

fn mb_store_or_err(
    state: &WebState,
) -> Result<&Arc<mailrs_mailbox::MailboxStore>, Value> {
    state.mailbox_store.as_ref().ok_or_else(|| {
        serde_json::json!({
            "type": "urn:ietf:params:jmap:error:serverUnavailable",
            "description": "mailbox store not available"
        })
    })
}

fn parse_email_db_id(email_id: &str) -> Option<i64> {
    email_id.strip_prefix("msg-").and_then(|s| s.parse().ok())
}

fn parse_mailbox_db_id(mb_id: &str) -> Option<i64> {
    mb_id.strip_prefix("mb-").and_then(|s| s.parse().ok())
}

async fn handle_mailbox_get(
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;
    let mailboxes = mb_store
        .list_mailboxes(&auth.address)
        .await
        .map_err(|e| serde_json::json!({
            "type": "urn:ietf:params:jmap:error:serverFail",
            "description": e.to_string()
        }))?;

    let requested_ids = args
        .get("ids")
        .and_then(|v| v.as_array());

    let mut list = Vec::new();
    let mut not_found = Vec::new();

    let iter_mailboxes: Vec<_> = if let Some(ids) = requested_ids {
        let id_strings: Vec<String> = ids
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let mut result = Vec::new();
        for id_str in &id_strings {
            if let Some(db_id) = parse_mailbox_db_id(id_str) {
                match mailboxes.iter().find(|m| m.id == db_id) {
                    Some(m) => result.push(m.clone()),
                    None => not_found.push(Value::String(id_str.clone())),
                }
            } else {
                not_found.push(Value::String(id_str.clone()));
            }
        }
        result
    } else {
        mailboxes.clone()
    };

    for mb in &iter_mailboxes {
        let (total, unseen) = mb_store
            .mailbox_status(mb.id)
            .await
            .unwrap_or((0, 0));

        let mb_id = format!("mb-{}", mb.id);
        let mut obj = serde_json::json!({
            "id": mb_id,
            "name": mb.name,
            "parentId": null,
            "sortOrder": 0,
            "totalEmails": total,
            "unreadEmails": unseen,
            "totalThreads": total,
            "unreadThreads": unseen,
            "myRights": {
                "mayReadItems": true,
                "mayAddItems": true,
                "mayRemoveItems": true,
                "maySetSeen": true,
                "maySetKeywords": true,
                "mayCreateChild": false,
                "mayRename": false,
                "mayDelete": false,
                "maySubmit": true
            },
            "isSubscribed": true
        });

        if let Some(role) = mailbox_role(&mb.name) {
            obj["role"] = Value::String(role.to_string());
        }

        list.push(obj);
    }

    Ok(("Mailbox/get".to_string(), serde_json::json!({
        "accountId": auth.address,
        "state": "0",
        "list": list,
        "notFound": not_found
    })))
}

async fn handle_mailbox_query(
    _args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;
    let mailboxes = mb_store
        .list_mailboxes(&auth.address)
        .await
        .map_err(|e| serde_json::json!({
            "type": "urn:ietf:params:jmap:error:serverFail",
            "description": e.to_string()
        }))?;

    let ids: Vec<String> = mailboxes
        .iter()
        .map(|mb| format!("mb-{}", mb.id))
        .collect();

    Ok(("Mailbox/query".to_string(), serde_json::json!({
        "accountId": auth.address,
        "queryState": "0",
        "canCalculateChanges": false,
        "position": 0,
        "total": ids.len(),
        "ids": ids
    })))
}

async fn handle_email_get(
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;

    let ids = args
        .get("ids")
        .and_then(|v| v.as_array())
        .ok_or_else(|| serde_json::json!({
            "type": "urn:ietf:params:jmap:error:invalidArguments",
            "description": "ids is required"
        }))?;

    let properties = args
        .get("properties")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
        });

    let mut list = Vec::new();
    let mut not_found = Vec::new();

    for id_val in ids {
        let id_str = match id_val.as_str() {
            Some(s) => s,
            None => continue,
        };

        let db_id = match parse_email_db_id(id_str) {
            Some(id) => id,
            None => {
                not_found.push(Value::String(id_str.to_string()));
                continue;
            }
        };

        let msg = match mb_store.get_message_by_db_id(&auth.address, db_id).await {
            Ok(Some(m)) => m,
            _ => {
                not_found.push(Value::String(id_str.to_string()));
                continue;
            }
        };

        let email = build_email_object(&msg, id_str, &properties, state);
        list.push(email);
    }

    Ok(("Email/get".to_string(), serde_json::json!({
        "accountId": auth.address,
        "state": "0",
        "list": list,
        "notFound": not_found
    })))
}

fn build_email_object(
    msg: &mailrs_mailbox::types::MessageMeta,
    email_id: &str,
    properties: &Option<Vec<&str>>,
    state: &WebState,
) -> Value {
    let include_all = properties.is_none();
    let props = properties.as_ref();

    let wants = |name: &str| include_all || props.is_some_and(|p| p.contains(&name));

    let mut obj = serde_json::Map::new();
    obj.insert("id".to_string(), Value::String(email_id.to_string()));

    if wants("threadId") {
        obj.insert(
            "threadId".to_string(),
            Value::String(msg.thread_id.clone()),
        );
    }

    if wants("mailboxIds") {
        let mb_key = format!("mb-{}", msg.mailbox_id);
        obj.insert(
            "mailboxIds".to_string(),
            serde_json::json!({ mb_key: true }),
        );
    }

    if wants("keywords") {
        obj.insert("keywords".to_string(), flags_to_keywords(msg.flags));
    }

    if wants("from") {
        obj.insert("from".to_string(), parse_address_list(&msg.sender));
    }

    if wants("to") {
        obj.insert("to".to_string(), parse_address_list(&msg.recipients));
    }

    if wants("subject") {
        obj.insert("subject".to_string(), Value::String(msg.subject.clone()));
    }

    if wants("receivedAt") {
        obj.insert(
            "receivedAt".to_string(),
            Value::String(epoch_to_utc_string(msg.internal_date)),
        );
    }

    if wants("sentAt") {
        obj.insert(
            "sentAt".to_string(),
            Value::String(epoch_to_utc_string(msg.date)),
        );
    }

    if wants("size") {
        obj.insert("size".to_string(), Value::Number(msg.size.into()));
    }

    if wants("messageId") {
        obj.insert(
            "messageId".to_string(),
            serde_json::json!([msg.message_id]),
        );
    }

    if wants("inReplyTo") {
        if msg.in_reply_to.is_empty() {
            obj.insert("inReplyTo".to_string(), Value::Null);
        } else {
            obj.insert(
                "inReplyTo".to_string(),
                serde_json::json!([msg.in_reply_to]),
            );
        }
    }

    if wants("preview") {
        let preview = msg
            .new_content
            .as_deref()
            .unwrap_or(&msg.subject)
            .chars()
            .take(256)
            .collect::<String>();
        obj.insert("preview".to_string(), Value::String(preview));
    }

    if wants("hasAttachment") {
        obj.insert("hasAttachment".to_string(), Value::Bool(false));
    }

    if wants("bodyValues") || wants("textBody") || wants("htmlBody") || wants("attachments") {
        if let Some(raw) =
            crate::message_util::read_message_raw(&state.maildir_root, &msg.user_address, &msg.maildir_id)
        {
            let (text_body, html_body, attachments) = crate::message_util::parse_message(&raw);

            if wants("bodyValues") {
                let mut bv = serde_json::Map::new();
                if let Some(ref text) = text_body {
                    bv.insert(
                        "t1".to_string(),
                        serde_json::json!({"value": text, "isEncodingProblem": false, "isTruncated": false}),
                    );
                }
                if let Some(ref html) = html_body {
                    bv.insert(
                        "h1".to_string(),
                        serde_json::json!({"value": html, "isEncodingProblem": false, "isTruncated": false}),
                    );
                }
                obj.insert("bodyValues".to_string(), Value::Object(bv));
            }

            if wants("textBody") {
                if text_body.is_some() {
                    obj.insert(
                        "textBody".to_string(),
                        serde_json::json!([{"partId": "t1", "type": "text/plain"}]),
                    );
                } else {
                    obj.insert("textBody".to_string(), serde_json::json!([]));
                }
            }

            if wants("htmlBody") {
                if html_body.is_some() {
                    obj.insert(
                        "htmlBody".to_string(),
                        serde_json::json!([{"partId": "h1", "type": "text/html"}]),
                    );
                } else {
                    obj.insert("htmlBody".to_string(), serde_json::json!([]));
                }
            }

            if wants("attachments") {
                let att_list: Vec<Value> = attachments
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "name": a.filename,
                            "type": a.content_type,
                            "size": a.size
                        })
                    })
                    .collect();
                obj.insert("attachments".to_string(), Value::Array(att_list));
                obj.insert(
                    "hasAttachment".to_string(),
                    Value::Bool(!attachments.is_empty()),
                );
            }
        } else {
            if wants("bodyValues") {
                obj.insert("bodyValues".to_string(), serde_json::json!({}));
            }
            if wants("textBody") {
                obj.insert("textBody".to_string(), serde_json::json!([]));
            }
            if wants("htmlBody") {
                obj.insert("htmlBody".to_string(), serde_json::json!([]));
            }
            if wants("attachments") {
                obj.insert("attachments".to_string(), serde_json::json!([]));
            }
        }
    }

    Value::Object(obj)
}

fn parse_address_list(raw: &str) -> Value {
    if raw.is_empty() {
        return Value::Array(vec![]);
    }
    let addrs: Vec<Value> = raw
        .split(',')
        .map(|s| {
            let s = s.trim();
            if let Some(pos) = s.find('<') {
                let name = s[..pos].trim().trim_matches('"').to_string();
                let email = s[pos + 1..].trim_end_matches('>').to_string();
                serde_json::json!({"name": name, "email": email})
            } else {
                serde_json::json!({"name": null, "email": s})
            }
        })
        .collect();
    Value::Array(addrs)
}

fn epoch_to_utc_string(epoch: i64) -> String {
    use chrono::{DateTime, Utc};
    DateTime::<Utc>::from_timestamp(epoch, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

async fn handle_email_query(
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;

    let filter = args.get("filter");

    let mailbox_id_filter = filter
        .and_then(|f| f.get("inMailbox"))
        .and_then(|v| v.as_str())
        .and_then(parse_mailbox_db_id);

    let text_filter = filter
        .and_then(|f| f.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    let has_keyword = filter
        .and_then(|f| f.get("hasKeyword"))
        .and_then(|v| v.as_str());

    let not_keyword = filter
        .and_then(|f| f.get("notKeyword"))
        .and_then(|v| v.as_str());

    let position = args
        .get("position")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(500) as u32;

    let sort_desc = args
        .get("sort")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.get("isAscending"))
        .and_then(|v| v.as_bool())
        .map(|asc| !asc)
        .unwrap_or(true);

    let mailboxes = mb_store
        .list_mailboxes(&auth.address)
        .await
        .map_err(|e| serde_json::json!({
            "type": "urn:ietf:params:jmap:error:serverFail",
            "description": e.to_string()
        }))?;

    let mailbox_ids: Vec<i64> = if let Some(mb_id) = mailbox_id_filter {
        if mailboxes.iter().any(|m| m.id == mb_id) {
            vec![mb_id]
        } else {
            vec![]
        }
    } else {
        mailboxes.iter().map(|m| m.id).collect()
    };

    let mut all_messages = Vec::new();
    for mb_id in &mailbox_ids {
        let msgs = mb_store
            .list_messages(*mb_id, 0, 10_000)
            .await
            .unwrap_or_default();
        all_messages.extend(msgs);
    }

    // apply keyword filters
    if let Some(kw) = has_keyword {
        let flag_bit = keyword_to_flag(kw);
        if flag_bit != 0 {
            all_messages.retain(|m| m.flags & flag_bit != 0);
        }
    }
    if let Some(kw) = not_keyword {
        let flag_bit = keyword_to_flag(kw);
        if flag_bit != 0 {
            all_messages.retain(|m| m.flags & flag_bit == 0);
        }
    }

    // apply text filter
    if let Some(ref query) = text_filter {
        all_messages.retain(|m| {
            m.subject.to_lowercase().contains(query)
                || m.sender.to_lowercase().contains(query)
                || m.recipients.to_lowercase().contains(query)
        });
    }

    // sort by date
    if sort_desc {
        all_messages.sort_by(|a, b| b.internal_date.cmp(&a.internal_date));
    } else {
        all_messages.sort_by(|a, b| a.internal_date.cmp(&b.internal_date));
    }

    let total = all_messages.len();
    let ids: Vec<String> = all_messages
        .iter()
        .skip(position as usize)
        .take(limit as usize)
        .map(|m| format!("msg-{}", m.id))
        .collect();

    Ok(("Email/query".to_string(), serde_json::json!({
        "accountId": auth.address,
        "queryState": "0",
        "canCalculateChanges": false,
        "position": position,
        "total": total,
        "ids": ids
    })))
}

fn keyword_to_flag(keyword: &str) -> u32 {
    match keyword {
        "$seen" => FLAG_SEEN,
        "$answered" => FLAG_ANSWERED,
        "$flagged" => FLAG_FLAGGED,
        "$draft" => FLAG_DRAFT,
        _ => 0,
    }
}

async fn handle_email_set(
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;

    let mut updated = serde_json::Map::new();
    let mut destroyed: Vec<String> = Vec::new();
    let mut not_updated = serde_json::Map::new();
    let mut not_destroyed = serde_json::Map::new();

    // handle updates (keyword changes)
    if let Some(updates) = args.get("update").and_then(|v| v.as_object()) {
        for (email_id, patch) in updates {
            let db_id = match parse_email_db_id(email_id) {
                Some(id) => id,
                None => {
                    not_updated.insert(
                        email_id.clone(),
                        serde_json::json!({"type": "notFound"}),
                    );
                    continue;
                }
            };

            let msg = match mb_store.get_message_by_db_id(&auth.address, db_id).await {
                Ok(Some(m)) => m,
                _ => {
                    not_updated.insert(
                        email_id.clone(),
                        serde_json::json!({"type": "notFound"}),
                    );
                    continue;
                }
            };

            let new_flags = if let Some(kw) = patch.get("keywords") {
                keywords_to_flags(kw)
            } else {
                // handle individual keyword patches like "keywords/$seen" => true/false
                let mut flags = msg.flags;
                if let Some(obj) = patch.as_object() {
                    for (key, val) in obj {
                        if let Some(kw_name) = key.strip_prefix("keywords/") {
                            let flag_bit = keyword_to_flag(kw_name);
                            if flag_bit != 0 {
                                if val.as_bool() == Some(true) {
                                    flags |= flag_bit;
                                } else {
                                    flags &= !flag_bit;
                                }
                            }
                        }
                    }
                }
                flags
            };

            match mb_store.update_flags(msg.mailbox_id, msg.uid, new_flags).await {
                Ok(_) => {
                    updated.insert(email_id.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(
                        email_id.clone(),
                        serde_json::json!({"type": "serverFail", "description": e.to_string()}),
                    );
                }
            }
        }
    }

    // handle destroy (delete)
    if let Some(destroy_ids) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_ids {
            let email_id = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };

            let db_id = match parse_email_db_id(email_id) {
                Some(id) => id,
                None => {
                    not_destroyed.insert(
                        email_id.to_string(),
                        serde_json::json!({"type": "notFound"}),
                    );
                    continue;
                }
            };

            let msg = match mb_store.get_message_by_db_id(&auth.address, db_id).await {
                Ok(Some(m)) => m,
                _ => {
                    not_destroyed.insert(
                        email_id.to_string(),
                        serde_json::json!({"type": "notFound"}),
                    );
                    continue;
                }
            };

            match mb_store
                .add_flags(msg.mailbox_id, msg.uid, FLAG_DELETED)
                .await
            {
                Ok(_) => destroyed.push(email_id.to_string()),
                Err(e) => {
                    not_destroyed.insert(
                        email_id.to_string(),
                        serde_json::json!({"type": "serverFail", "description": e.to_string()}),
                    );
                }
            }
        }
    }

    Ok(("Email/set".to_string(), serde_json::json!({
        "accountId": auth.address,
        "oldState": "0",
        "newState": "1",
        "updated": updated,
        "destroyed": destroyed,
        "notUpdated": not_updated,
        "notDestroyed": not_destroyed
    })))
}

async fn handle_thread_get(
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;

    let ids = args
        .get("ids")
        .and_then(|v| v.as_array())
        .ok_or_else(|| serde_json::json!({
            "type": "urn:ietf:params:jmap:error:invalidArguments",
            "description": "ids is required"
        }))?;

    let mut list = Vec::new();
    let mut not_found = Vec::new();

    for id_val in ids {
        let thread_id = match id_val.as_str() {
            Some(s) => s,
            None => continue,
        };

        let messages = mb_store
            .list_thread_messages(&auth.address, thread_id, None)
            .await
            .unwrap_or_default();

        if messages.is_empty() {
            not_found.push(Value::String(thread_id.to_string()));
            continue;
        }

        let email_ids: Vec<String> = messages
            .iter()
            .map(|m| format!("msg-{}", m.id))
            .collect();

        list.push(serde_json::json!({
            "id": thread_id,
            "emailIds": email_ids
        }));
    }

    Ok(("Thread/get".to_string(), serde_json::json!({
        "accountId": auth.address,
        "state": "0",
        "list": list,
        "notFound": not_found
    })))
}

async fn handle_email_submission_set(
    args: &Value,
    auth: &AuthUser,
    state: &Arc<WebState>,
) -> Result<(String, Value), Value> {
    let mb_store = mb_store_or_err(state)?;

    let create = match args.get("create").and_then(|v| v.as_object()) {
        Some(c) => c,
        None => {
            return Ok(("EmailSubmission/set".to_string(), serde_json::json!({
                "accountId": auth.address,
                "oldState": "0",
                "newState": "0",
                "created": {},
                "notCreated": {}
            })));
        }
    };

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();

    for (creation_id, submission) in create {
        let email_id = match submission.get("emailId").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => {
                not_created.insert(
                    creation_id.clone(),
                    serde_json::json!({
                        "type": "invalidProperties",
                        "description": "emailId is required"
                    }),
                );
                continue;
            }
        };

        let db_id = match parse_email_db_id(email_id) {
            Some(id) => id,
            None => {
                not_created.insert(
                    creation_id.clone(),
                    serde_json::json!({"type": "invalidProperties", "description": "invalid emailId"}),
                );
                continue;
            }
        };

        let msg = match mb_store.get_message_by_db_id(&auth.address, db_id).await {
            Ok(Some(m)) => m,
            _ => {
                not_created.insert(
                    creation_id.clone(),
                    serde_json::json!({"type": "notFound", "description": "email not found"}),
                );
                continue;
            }
        };

        let raw = match crate::message_util::read_message_raw(
            &state.maildir_root,
            &auth.address,
            &msg.maildir_id,
        ) {
            Some(r) => r,
            None => {
                not_created.insert(
                    creation_id.clone(),
                    serde_json::json!({"type": "serverFail", "description": "could not read message"}),
                );
                continue;
            }
        };

        let to: Vec<String> = msg
            .recipients
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let Json(result) = crate::web::mail::deliver_message(
            state,
            &auth.address,
            &to,
            &[],
            &[],
            &raw,
            &msg.message_id,
            msg.date,
        )
        .await;

        if result.success {
            created.insert(
                creation_id.clone(),
                serde_json::json!({
                    "id": format!("sub-{}", db_id),
                    "emailId": email_id,
                    "undoStatus": "final"
                }),
            );
        } else {
            let err_msg = result.message.as_deref().unwrap_or("delivery failed");
            not_created.insert(
                creation_id.clone(),
                serde_json::json!({"type": "serverFail", "description": err_msg}),
            );
        }
    }

    Ok(("EmailSubmission/set".to_string(), serde_json::json!({
        "accountId": auth.address,
        "oldState": "0",
        "newState": "1",
        "created": created,
        "notCreated": not_created
    })))
}
