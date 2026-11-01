//! Record every file each user's copy of a message already has.
//!
//! A delete unlinks the files in `mailrs:usermsg-files:{user}:{mid}`, and
//! `upsert_user_message` adds to that set from now on. Mail filed before it
//! existed has only the one name its row carries, while the same Message-ID
//! can sit in a mailbox several times — Sent and INBOX, or INBOX once per
//! alias it came through. This walks each mailbox once, reads each file's
//! Message-ID, and adds every file of a message the user has a row for.
//!
//! Files with no row are counted and left alone: they are mail nobody's
//! index knows, and what happens to them is the owner's call, not a
//! backfill's. Compares before it writes, so a second run reports
//! `added: 0`.

use std::collections::BTreeMap;

use super::prelude::*;

#[derive(serde::Deserialize, Default)]
pub(crate) struct BackfillFilesQuery {
    #[serde(default)]
    dry_run: bool,
    user: Option<String>,
}

/// `POST /v1/admin/maintenance:backfill-message-files[?user=&dry_run=true]`
pub(crate) async fn backfill_message_files_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillFilesQuery>,
) -> axum::response::Response {
    let users = match &q.user {
        Some(u) => vec![u.clone()],
        None => match state.mailbox.list_account_addresses() {
            Ok(u) => u,
            Err(e) => {
                tracing::error!(err = %e, "list_account_addresses failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let dry_run = q.dry_run;
    let report =
        super::hangup::run_blocking(move |h| walk(&state, &root, &users, dry_run, h)).await;
    match report {
        Ok(Ok(r)) => Json(r).into_response(),
        Ok(Err(e)) => {
            tracing::error!(err = %e, "backfill-message-files failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, "backfill-message-files task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn walk(
    state: &Arc<FastcoreState>,
    root: &str,
    users: &[String],
    dry_run: bool,
    hangup: &super::hangup::Hangup,
) -> std::io::Result<serde_json::Value> {
    let mut total = Counts::default();
    let mut by_user: BTreeMap<String, Counts> = BTreeMap::new();
    let mut stopped_early = false;
    for user in users {
        let mut c = Counts::default();
        for (mid, files) in user_files_by_message_id(root, user) {
            if hangup.hung_up() {
                tracing::warn!(user = %user, "backfill-message-files: the caller hung up, stopping");
                stopped_early = true;
                break;
            }
            c.walked += files.len() as u64;
            let Some(facts) = state.mailbox.user_message_facts(user, &mid)? else {
                c.no_row += files.len() as u64;
                continue;
            };
            // the owner index reads `tid` off the row; a row without one
            // is invisible to it, and a delete would think it unowned
            if facts.tid.is_empty() {
                c.rows_without_tid += 1;
            }
            if files.len() > 1 {
                c.messages_with_several_files += 1;
            }
            for file in &files {
                if state.mailbox.has_message_file(user, &mid, file)? {
                    c.already += 1;
                    continue;
                }
                if !dry_run {
                    state.mailbox.add_message_file(user, &mid, file)?;
                }
                c.added += 1;
            }
        }
        total.add(&c);
        if c.walked > 0 {
            by_user.insert(user.clone(), c);
        }
        if stopped_early {
            break;
        }
    }
    Ok(serde_json::json!({
        "dry_run": dry_run,
        "stopped_early": stopped_early,
        "users": users.len(),
        "total": total,
        "by_user": by_user,
    }))
}

#[derive(Default, serde::Serialize)]
struct Counts {
    walked: u64,
    added: u64,
    already: u64,
    no_row: u64,
    messages_with_several_files: u64,
    rows_without_tid: u64,
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        self.walked += o.walked;
        self.added += o.added;
        self.already += o.already;
        self.no_row += o.no_row;
        self.messages_with_several_files += o.messages_with_several_files;
        self.rows_without_tid += o.rows_without_tid;
    }
}

#[cfg(test)]
mod tests {
    use super::super::hangup::Hangup;
    use super::*;

    #[test]
    fn a_walk_stops_soon_after_the_caller_hangs_up() {
        let state = crate::tests::fresh_state();
        let tmp = tempfile::tempdir().unwrap();
        let cur = tmp.path().join("x/u/cur");
        std::fs::create_dir_all(&cur).unwrap();
        for i in 0..5 {
            std::fs::write(
                cur.join(format!("1700000000.M{i}P1.host:2,S")),
                format!("Message-ID: <m{i}@x>\r\nSubject: s\r\n\r\nbody\r\n"),
            )
            .unwrap();
        }
        let root = tmp.path().to_str().unwrap();
        let users = vec!["u@x".to_string()];
        let all = walk(&state, root, &users, true, &Hangup::never()).unwrap();
        assert_eq!(all["total"]["walked"], 5, "{all}");
        assert_eq!(all["stopped_early"], false, "{all}");
        let cut = walk(&state, root, &users, true, &Hangup::after(2)).unwrap();
        assert_eq!(cut["total"]["walked"], 2, "{cut}");
        assert_eq!(cut["stopped_early"], true, "{cut}");
    }
}
