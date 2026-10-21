//! The SQL `list_conversations` sends: its WHERE and HAVING clauses, built
//! from the filter axes, and the projection around them. Placeholders are
//! numbered in the order the caller binds.

const COUNT_EXPR: &str =
    "COUNT(DISTINCT CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END)";
const UNREAD_EXPR: &str = "COUNT(DISTINCT CASE WHEN (m.flags & 1) = 0 THEN CASE WHEN m.message_id != '' THEN m.message_id ELSE CAST(m.id AS TEXT) END END)";

// The thread's effective category = the latest message's
// `email_analysis.category` (defaulting 'general' when the
// newest message has no analysis row). Reused by the projection
// AND the v2.9 triage-bucket folder filter below so the two
// never disagree.
const THREAD_CAT: &str = "COALESCE((SELECT ea_cat.category FROM email_analysis ea_cat \
     JOIN messages m_cat ON ea_cat.message_id = m_cat.id \
     WHERE m_cat.thread_id = m.thread_id \
     ORDER BY m_cat.internal_date DESC LIMIT 1), 'general')";

/// The filter axes of one `list_conversations` call.
pub(super) struct Filters<'a> {
    pub(super) before_ts: Option<i64>,
    pub(super) category: Option<&'a str>,
    pub(super) domains: Option<&'a [String]>,
    pub(super) archived: bool,
    pub(super) folder: Option<&'a str>,
    pub(super) unread: Option<bool>,
    pub(super) starred: Option<bool>,
    pub(super) section: Option<&'a str>,
}

pub(super) struct ListQuery {
    pub(super) sql: String,
    /// Whether `$folder` is one of the bound parameters.
    pub(super) folder_binds_name: bool,
}

/// What the folder axis adds besides its WHERE condition.
struct FolderFilter {
    binds_name: bool,
    bucket_having: Option<String>,
    // Inbound triage buckets (Inbox/N/P/NP) exclude Sent-only threads
    // just like the "All" view — a thread the user only ever sent
    // belongs to the Sent axis alone, never to Inbox. Mirrors the
    // fastcore `count > sent_count` guard.
    inbound_bucket: bool,
}

impl Filters<'_> {
    pub(super) fn query(&self) -> ListQuery {
        // build dynamic WHERE clauses
        let mut conditions = vec!["thread_id != ''".to_string()];
        let mut param_idx = 1u32;

        // user filter: either single user or multi-domain
        let user_condition = self.user_condition(&mut param_idx);
        conditions.insert(0, user_condition);

        // Hide an active snooze from the inbox, but NOT from Archived.
        //
        // This condition used to be unconditional, so a snoozed thread was
        // excluded from every view — including the archived one — and could not
        // be found anywhere until its time passed. A conversation somebody put
        // away for a week was, for that week, gone.
        //
        // Asleep means filed away, and the client says so in as many words:
        // "a thread that is asleep is filed away, so it shows up in Archived and
        // nowhere else. Without saying when it comes back, that reads as an
        // ordinary archived thread and the way out is not obvious"
        // (`web/src/components/conversation-item.tsx`). It renders a "Snoozed
        // until <date>" badge on the row to tell the two apart — which needs the
        // row to be there.
        //
        // The badge itself is still blank on this lane: `snoozed_until` is
        // hard-coded to 0 in the projection, and adding the column does not
        // compile because sqlx implements `FromRow` for tuples up to sixteen
        // elements and this projection uses all sixteen.
        // Reachable-but-unlabelled is a
        // strictly better place to be stuck than unreachable.
        if !self.archived {
            conditions.push(
                "NOT EXISTS (SELECT 1 FROM snoozed_conversations sc WHERE sc.thread_id = m.thread_id AND sc.account_address = mb.user_address AND sc.snoozed_until > NOW())".to_string()
            );
        }

        let folder = self.folder_filter(&mut conditions, &mut param_idx);

        let limit_idx = param_idx;
        param_idx += 1;

        if self.before_ts.is_some() {
            conditions.push(format!("internal_date < ${param_idx}"));
            param_idx += 1;
        }
        // perf: SubPlan 5 + 8 (per-row lookups on email_analysis for
        // requires_action and spam/scam exclusion) collapsed into a single
        // LEFT JOIN. one merge/hash join instead of ~36k index probes
        // per request.
        // "all" must mean ALL: the default view never hides mail by
        // category. category is an opt-in filter only (pass a category to
        // narrow to it, e.g. a Spam folder). nothing silently disappears
        // from the default list because a (possibly stale, possibly
        // wrong) classifier tagged it spam/scam. (2026-06-13: legit
        // business mail — POs, settlement agreements — was found hidden
        // this way after AI was disabled but old categories persisted.)
        if self.category.is_some() {
            conditions.push(format!("ea.category = ${param_idx}"));
        }

        let where_clause = conditions.join(" AND ");
        let having_clause = self.having_clause(&folder);
        ListQuery {
            sql: select_sql(&where_clause, &having_clause, limit_idx),
            folder_binds_name: folder.binds_name,
        }
    }

    fn user_condition(&self, param_idx: &mut u32) -> String {
        if let Some(doms) = self.domains {
            if doms.is_empty() {
                *param_idx += 1;
                format!("mb.user_address = ${}", *param_idx - 1)
            } else {
                let placeholders: Vec<String> = doms
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", *param_idx + i as u32))
                    .collect();
                *param_idx += doms.len() as u32;
                format!(
                    "mb.user_address IN (SELECT address FROM accounts WHERE domain IN ({}))",
                    placeholders.join(",")
                )
            }
        } else {
            *param_idx += 1;
            format!("mb.user_address = ${}", *param_idx - 1)
        }
    }

    // folder axis (v2.9 triage). `Sent` stays an IMAP mailbox-name
    // match (orthogonal axis). The triage buckets
    // (Inbox/Notifications/Promotions/NP/Junk) are a pure function
    // of the thread's category — mirroring the fastcore/kevy
    // `bucket_of` so both lanes behave identically. Bucket filters
    // go in HAVING (they reference the per-thread category), never
    // as a bound param.
    fn folder_filter(&self, conditions: &mut Vec<String>, param_idx: &mut u32) -> FolderFilter {
        let thread_cat = THREAD_CAT;
        let mut out = FolderFilter {
            binds_name: false,
            bucket_having: None,
            inbound_bucket: false,
        };
        match self.folder {
            Some(f) if f.eq_ignore_ascii_case("sent") => {
                conditions.push(format!("mb.name = ${param_idx}"));
                *param_idx += 1;
                out.binds_name = true;
            }
            Some(f) if f.eq_ignore_ascii_case("inbox") => {
                out.inbound_bucket = true;
                // Inbox = not a triage bucket AND not in the Junk
                // mailbox (ingest routes spam to the Junk mailbox before
                // the async classifier stamps category — cover both).
                out.bucket_having = Some(format!(
                    "{thread_cat} NOT IN ('spam','scam','notification','promotion') \
                     AND BOOL_OR(mb.name = 'Junk') = false"
                ));
            }
            Some(f) if f.eq_ignore_ascii_case("notifications") => {
                out.inbound_bucket = true;
                out.bucket_having = Some(format!("{thread_cat} = 'notification'"));
            }
            Some(f) if f.eq_ignore_ascii_case("promotions") => {
                out.inbound_bucket = true;
                out.bucket_having = Some(format!("{thread_cat} = 'promotion'"));
            }
            Some(f) if f.eq_ignore_ascii_case("np") => {
                out.inbound_bucket = true;
                out.bucket_having = Some(format!("{thread_cat} IN ('notification','promotion')"));
            }
            Some(f) if f.eq_ignore_ascii_case("junk") => {
                // Junk = category-classified spam/scam OR delivered into
                // the Junk mailbox by the SMTP-time antispam gate.
                out.bucket_having = Some(format!(
                    "({thread_cat} IN ('spam','scam') OR BOOL_OR(mb.name = 'Junk') = true)"
                ));
            }
            // Legacy mailbox names (Drafts/Trash/…) keep the old
            // mailbox-name match; unknown folder falls through here too.
            Some(_) => {
                conditions.push(format!("mb.name = ${param_idx}"));
                *param_idx += 1;
                out.binds_name = true;
            }
            None => {}
        }
        out
    }

    // build HAVING clause with optional filters
    fn having_clause(&self, folder: &FolderFilter) -> String {
        let unread_expr = UNREAD_EXPR;
        let archived_filter = if self.archived {
            "BOOL_OR(m.archived) = true"
        } else {
            "BOOL_OR(m.archived) = false"
        };
        let mut having_parts = vec![archived_filter.to_string()];
        // For "All" / no-folder view, only include threads that have at
        // least one message NOT in the Sent mailbox. This keeps Sent-only
        // threads (drafts the user dispatched without any reply) out of the
        // default reading list, while still letting threads with both
        // inbound and outbound messages appear in BOTH All AND Sent —
        // exactly what the user expects of a conversation grouping.
        if self.folder.is_none() || folder.inbound_bucket {
            having_parts.push("BOOL_OR(mb.name != 'Sent') = true".to_string());
        }
        // v2.9 triage — the bucket folder filter (references the
        // per-thread category, so it must be HAVING not WHERE).
        if let Some(bh) = &folder.bucket_having {
            having_parts.push(bh.clone());
        }
        if self.unread == Some(true) {
            having_parts.push(format!("{unread_expr} > 0"));
        }
        if self.starred == Some(true) {
            having_parts.push("BOOL_OR((m.flags & 4) != 0) = true".to_string());
        }
        match self.section {
            // perf: ea.requires_action comes from the LEFT JOIN above
            Some("action") => having_parts
                .push("COALESCE(BOOL_OR(ea.requires_action), false) = true".to_string()),
            // perf: ordered aggregate replaces a per-group SubPlan that ran
            // a LIMIT-1 query on messages for each thread.
            // matches the SELECT-list expression so PG computes it once.
            Some("important") => having_parts.push(
                "COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal') IN ('critical', 'important')".to_string()
            ),
            Some("other") => having_parts.push(
                "COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal') IN ('low', 'noise')".to_string()
            ),
            _ => {}
        }
        having_parts.join(" AND ")
    }
}

fn select_sql(where_clause: &str, having_clause: &str, limit_idx: u32) -> String {
    let count_expr = COUNT_EXPR;
    let unread_expr = UNREAD_EXPR;
    // Display time + list position follow the last INBOUND message —
    // the user's own reply (Sent mailbox) must not re-date the row
    // (2026-07-18; mirrors the fastcore `is_own` arrival rule).
    // Sent-only threads fall back to the plain max.
    let display_date_expr =
        "COALESCE(MAX(CASE WHEN mb.name != 'Sent' THEN m.internal_date END), MAX(m.internal_date))";

    // The sort ends on `m.thread_id` so that it is TOTAL.
    //
    // `internal_date` is whole seconds, so threads that arrive in the same
    // second tie, and the order between tied rows was whatever the planner
    // happened to produce — undefined between two calls with the same
    // arguments. A paged reader sees that as a row skipped or repeated at a
    // boundary, intermittently; the kevy lane closed it with a folded-hash `ord` column
    // after measuring 929 collisions over 30k rows on prod, and this side
    // had no tie-break at all. Found by the cross-lane comparison: the two
    // cores agreed on every row and every field, and disagreed only *within*
    // a same-second group.
    //
    // `thread_id` rather than a hash of it: kevy needs the hash because a
    // composite index there drops any row whose string component exceeds
    // 255 bytes, and SQL has no such limit.

    let sql = format!(
            "SELECT m.thread_id, MAX(m.subject), string_agg(DISTINCT m.sender, ','),
                    {count_expr}, {unread_expr}, {display_date_expr},
                    COALESCE((SELECT ea.category FROM email_analysis ea
                              JOIN messages m2 ON ea.message_id = m2.id
                              WHERE m2.thread_id = m.thread_id
                              ORDER BY m2.internal_date DESC LIMIT 1), 'general'),
                    BOOL_OR((m.flags & 4) != 0),
                    COALESCE(
                      (SELECT LEFT(ea_snip.summary, 80) FROM email_analysis ea_snip
                       JOIN messages m_snip ON ea_snip.message_id = m_snip.id
                       WHERE m_snip.thread_id = m.thread_id AND ea_snip.summary IS NOT NULL AND ea_snip.summary != ''
                       ORDER BY m_snip.internal_date DESC LIMIT 1),
                      (SELECT LEFT(m3.text_body, 80) FROM messages m3
                       WHERE m3.thread_id = m.thread_id AND m3.text_body IS NOT NULL AND m3.text_body != ''
                       ORDER BY m3.internal_date DESC LIMIT 1),
                      ''),
                    BOOL_OR(m.pinned),
                    BOOL_OR(m.archived),
                    COALESCE((array_agg(m.importance_level ORDER BY m.importance_score DESC NULLS LAST))[1], 'normal'),
                    COALESCE(MAX(m.importance_score), 0.0),
                    COALESCE(BOOL_OR(ea.requires_action), false),
                    COALESCE((array_agg(m.sender ORDER BY m.internal_date DESC))[1], ''),
                    COUNT(DISTINCT CASE WHEN mb.name  = 'Sent' AND m.message_id != '' THEN m.message_id WHEN mb.name  = 'Sent' THEN CAST(m.id AS TEXT) END)
             FROM messages m
                  JOIN mailboxes mb ON m.mailbox_id = mb.id
                  LEFT JOIN email_analysis ea ON ea.message_id = m.id
             WHERE {where_clause}
             GROUP BY m.thread_id HAVING {having_clause}
             ORDER BY BOOL_OR(m.pinned) DESC, {display_date_expr} DESC, m.thread_id
             LIMIT ${limit_idx}"
        );
    sql
}
