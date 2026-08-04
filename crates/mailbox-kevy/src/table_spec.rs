//! The declared `threaduser` table, and the admin indexes.
//!
//! A query axis is **declared**, never maintained by hand
//! (`.claude/rules/kevy-patterns.md` → `kevy/declare-dont-maintain`). The
//! spec is its own function so the column/orderpath agreement can be
//! asserted: kevy panics rather than returning an error when an ORDERPATH
//! names a column the table never declared, and that panic is at boot.

use std::io;

use super::KevyMailboxStore;
use super::keys;

impl KevyMailboxStore {
    /// Declare the `threaduser` table — the access paths the engine
    /// maintains in place of twelve hand-written zsets.
    ///
    /// Idempotent: a second declaration of the same spec is refused by
    /// the catalog, which persisted it on the first call. Declaration
    /// is atomic — on any error nothing installs, so there is no
    /// half-declared table to clean up.
    ///
    /// **Nothing reads these paths yet.** The zsets remain
    /// authoritative through the backfill and the shadow-read window;
    /// this only starts the engine maintaining a parallel answer that
    /// `TABLE.VERIFY` can be checked against.
    ///
    /// Two ORDERPATHs cover the two orderings the UI actually asks for
    /// — the mutually-exclusive bucket (inbox / notifications /
    /// promotions / junk) and the finer category — both newest-first.
    /// The six orthogonal flags are stored as index VALUES instead of
    /// getting paths of their own, so "starred within inbox" is a
    /// FILTER over a path that already exists rather than a third
    /// composite.
    ///
    /// NOTE: `kevy-index` is imported directly because `kevy-embedded`
    /// 4.0.0 does not re-export `TableSpec` / `TableIndex` /
    /// `OrderPath`, which makes its own `Store::table_declare`
    /// uncallable through the facade. Drop this dependency once the
    /// re-export lands upstream.
    pub fn ensure_thread_table(&self) {
        let spec = thread_user_spec();

        // The catalog persists across boots, so a redeclaration is
        // rejected rather than applied — which would silently pin the
        // table to whatever shape the first boot happened to declare.
        // Compare and rebuild only on a real change.
        let current = self
            .store
            .table_list()
            .into_iter()
            .find(|t| t.name == spec.name);
        match current {
            Some(ref existing) if *existing == spec => {
                tracing::debug!("threaduser table already matches");
                return;
            }
            Some(_) => {
                tracing::info!("threaduser table spec changed — rebuilding indexes");
                self.store.table_drop(&spec.name);
            }
            None => {}
        }
        // Before the indexes are built, not after: they are built from
        // the rows, and a row missing a column an ORDERPATH keys on is in
        // none of them. `archived` joined those prefixes on 2026-08-05,
        // and the arrival path — which creates most rows — had never
        // written it, so declaring first would have served every account
        // an empty mailbox until a sweep caught up.
        match self.plant_missing_user_flags() {
            Ok((scanned, planted)) if planted > 0 => {
                tracing::info!(
                    scanned,
                    planted,
                    "membership rows given their missing flags"
                )
            }
            Ok((scanned, _)) => tracing::debug!(scanned, "membership rows already complete"),
            Err(e) => tracing::error!(error = %e, "planting membership row flags failed"),
        }
        match self.store.table_declare(spec) {
            Ok(()) => tracing::info!("threaduser table declared"),
            Err(e) => tracing::error!(error = %e, "threaduser table declaration failed"),
        }
    }

    /// Give every membership row the declared per-user flags it lacks.
    ///
    /// Returns `(scanned, planted)`. Convergent, not merely idempotent:
    /// a row that already carries all five is not written, so the second
    /// boot after a spec change reports `planted: 0` and touches nothing
    /// (`periodic-work-must-converge`).
    ///
    /// Only runs when the declaration is about to change, which is the
    /// only moment the set of columns an index needs can have grown.
    fn plant_missing_user_flags(&self) -> io::Result<(usize, usize)> {
        use crate::thread_row::PER_USER_FLAGS;
        let pattern = format!("{}*", String::from_utf8_lossy(keys::THREAD_USER_PREFIX));
        let mut scanned = 0usize;
        let mut planted = 0usize;
        for key in self.store.keys(Some(pattern.as_bytes()), None) {
            scanned += 1;
            let have: std::collections::HashSet<Vec<u8>> = self
                .store
                .hgetall(&key)
                .map_err(io::Error::other)?
                .into_iter()
                .map(|(f, _)| f)
                .collect();
            let missing: Vec<&str> = PER_USER_FLAGS
                .iter()
                .copied()
                .filter(|f| !have.contains(f.as_bytes()))
                .collect();
            if missing.is_empty() {
                continue;
            }
            let zeros: Vec<(&[u8], &[u8])> = missing
                .iter()
                .map(|f| (f.as_bytes(), b"0".as_slice()))
                .collect();
            self.store.hset(&key, &zeros).map_err(io::Error::other)?;
            planted += 1;
        }
        Ok((scanned, planted))
    }

    pub fn ensure_admin_indexes(&self) {
        use kevy_embedded::{IndexKind, IndexValType};
        let s = &self.store;
        // Full-text over the synthesised `search_blob` field. kevy
        // maintains this from its commit hook, so it cannot drift from
        // the rows the way an external search service does — which is
        // exactly how conversation search ended up dead for weeks
        // (index name mismatch + dropped writes, 2026-07-19).
        // Dictionary-free CJK bigrams, so Japanese subjects and sender
        // names are searchable without an analyzer.
        let _ = s.idx_create(
            keys::IDX_THREAD_SEARCH,
            keys::THREAD_PREFIX,
            keys::THREAD_SEARCH_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        // Message bodies, indexed separately from the thread rows so a
        // long conversation doesn't rewrite one ever-growing value on
        // every arrival.
        let _ = s.idx_create(
            keys::IDX_MESSAGE_TEXT,
            keys::MSGTEXT_PREFIX,
            keys::MESSAGE_TEXT_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        let _ = s.idx_create(
            keys::IDX_ALIASES_BY_DOMAIN,
            keys::ALIAS_V2_PREFIX,
            b"domain",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ALIASES_BY_TARGET,
            keys::ALIAS_V2_PREFIX,
            b"target",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_DOMAINS_BY_CREATED,
            keys::DOMAIN_V2_PREFIX,
            b"created_at",
            IndexValType::I64,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ACCOUNTS_BY_DOMAIN,
            keys::ACCOUNT_PREFIX,
            b"domain",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ACCOUNTS_BY_ACTIVE,
            keys::ACCOUNT_PREFIX,
            b"active",
            IndexValType::Str,
            IndexKind::Range,
        );
    }
}

/// The `threaduser` table declaration.
///
/// Split out of `ensure_thread_table` so the column/orderpath
/// agreement can be asserted in a test: kevy panics rather than
/// returning an error when an ORDERPATH names a column the table
/// never declared, and that panic happens at boot.
pub(crate) fn thread_user_spec() -> kevy_index::TableSpec {
    use kevy_index::{IndexKind, OrderPath, TableIndex, TableSpec, ValType};

    fn col(name: &str, t: ValType) -> (Vec<u8>, ValType) {
        (name.as_bytes().to_vec(), t)
    }
    fn path(name: &str, on: &[(&str, bool)]) -> OrderPath {
        OrderPath {
            name: name.as_bytes().to_vec(),
            on: on
                .iter()
                .map(|(c, desc)| (c.as_bytes().to_vec(), *desc))
                .collect(),
        }
    }
    // Stored alongside every flag index: `user` and `activity` scope
    // and order the axis, and the rest let a stacked predicate be a
    // FILTER on the same index rather than an intersection of several.
    // The UI stacks constantly — "archived within Inbox", "unread
    // within Inbox" — and those combinations have no index of their
    // own.
    let values: Vec<Vec<u8>> = [
        "user",
        "activity",
        "bucket",
        "category",
        "sent_only",
        "starred",
        "archived",
        "pinned",
        "unread",
        "has_action",
    ]
    .iter()
    .map(|c| c.as_bytes().to_vec())
    .collect();

    TableSpec {
        name: b"threaduser".to_vec(),
        prefix: keys::THREAD_USER_PREFIX.to_vec(),
        pk: b"tid".to_vec(),
        columns: vec![
            col("user", ValType::Str),
            col("tid", ValType::Str),
            col("ord", ValType::I64),
            col("bucket", ValType::Str),
            col("category", ValType::Str),
            col("activity", ValType::I64),
            col("sent_only", ValType::I64),
            col("is_sender", ValType::I64),
            col("starred", ValType::I64),
            col("archived", ValType::I64),
            col("pinned", ValType::I64),
            col("unread", ValType::I64),
            col("has_action", ValType::I64),
        ],
        // The boolean predicates, each keyed on its own flag with
        // `user` and `activity` stored alongside. Unlike the bucket
        // axes these are not a sort prefix: the query keys on the flag
        // and then filters to one user and sorts by activity through
        // the stored values, which is what keeps this to five small
        // indexes instead of five more composites.
        indexes: [
            "starred",
            "archived",
            "pinned",
            "unread",
            "has_action",
            // The Sent axis has the same shape: key on the flag,
            // filter to the user, sort by recency.
            "is_sender",
        ]
        .iter()
        .map(|c| TableIndex {
            column: c.as_bytes().to_vec(),
            kind: IndexKind::Range,
            values: values.clone(),
        })
        .collect(),
        // `ord` is the tie-breaker, not a queryable dimension.
        // `activity` is a whole-second timestamp, so threads that
        // arrive in the same second collide — 929 collisions over
        // 30k rows on prod. Without a total order the position of a
        // colliding row is undefined between calls, which is how a
        // paged reader silently skips or repeats one at a page
        // boundary.
        // `archived` sits in every prefix for the same reason
        // `sent_only` sits in one: Archived is a list of its own, so
        // every other list excludes it, and an exclusion belongs in the
        // declared query shape rather than in a filter applied to the
        // page after it was counted. Without it the server answered
        // "Inbox" with archived threads in it and the client deleted
        // them from the page it had already been told the size of.
        orderpaths: vec![
            path(
                "by_user_bucket",
                &[
                    ("user", false),
                    ("bucket", false),
                    ("archived", false),
                    ("activity", true),
                    ("ord", false),
                ],
            ),
            // Inbox is not simply `bucket = inbox`: a thread the user
            // only ever sent to belongs in Sent, not in the inbox.
            // Putting `sent` in the sort prefix makes that exclusion
            // part of the declared query shape rather than a filter
            // applied after the fact — on prod it is 72 threads for
            // one account that would otherwise show up in their own
            // inbox.
            path(
                "by_user_bucket_sent",
                &[
                    ("user", false),
                    ("bucket", false),
                    ("sent_only", false),
                    ("archived", false),
                    ("activity", true),
                    ("ord", false),
                ],
            ),
            // The default axis — no predicate but the archive
            // exclusion, just the user's threads newest first.
            path(
                "by_user_activity",
                &[
                    ("user", false),
                    ("archived", false),
                    ("activity", true),
                    ("ord", false),
                ],
            ),
            path(
                "by_user_category",
                &[
                    ("user", false),
                    ("category", false),
                    ("archived", false),
                    ("activity", true),
                    ("ord", false),
                ],
            ),
        ],
    }
}

#[cfg(test)]
mod table_spec_tests {
    use super::*;
    use crate::thread_row;

    /// Every column an ORDERPATH or index sorts on must be declared.
    ///
    /// kevy resolves these with `column_type(col).expect("validated")`
    /// while compiling the table, so an undeclared column is a panic on
    /// the boot path — the process restart-loops rather than reporting
    /// a bad declaration. This landed on prod once (v2.12.5, rolled
    /// back in minutes) when a column was added to an orderpath and not
    /// to `columns`.
    #[test]
    fn orderpath_columns_are_declared() {
        let spec = thread_user_spec();
        let declared: std::collections::BTreeSet<Vec<u8>> =
            spec.columns.iter().map(|(n, _)| n.clone()).collect();

        for op in &spec.orderpaths {
            for (col, _) in &op.on {
                assert!(
                    declared.contains(col),
                    "orderpath {} sorts on undeclared column {}",
                    String::from_utf8_lossy(&op.name),
                    String::from_utf8_lossy(col)
                );
            }
        }
        for ix in &spec.indexes {
            assert!(
                declared.contains(&ix.column),
                "index keys on undeclared column {}",
                String::from_utf8_lossy(&ix.column)
            );
            for v in &ix.values {
                assert!(
                    declared.contains(v),
                    "index on {} stores undeclared column {}",
                    String::from_utf8_lossy(&ix.column),
                    String::from_utf8_lossy(v)
                );
            }
        }
    }

    /// A flag index can be keyed on; it can only be *filtered* on when
    /// the other flag indexes store it beside their own key.
    ///
    /// `is_sender` is the one that they do not, which is why
    /// `ListThreadsFilter::flags_on` returns it first and the dispatcher
    /// keys on whatever comes back first. A second such column would
    /// break that — two of them could be asked for at once and only one
    /// can be the key — so this fails when one appears, rather than at
    /// runtime with "FILTER names field 'x', which this index does not
    /// store".
    #[test]
    fn is_sender_is_the_only_flag_that_must_be_the_key() {
        let spec = thread_user_spec();
        let mut key_only: Vec<String> = Vec::new();
        for ix in &spec.indexes {
            let stored_everywhere = spec
                .indexes
                .iter()
                .all(|other| other.column == ix.column || other.values.contains(&ix.column));
            if !stored_everywhere {
                key_only.push(String::from_utf8_lossy(&ix.column).into_owned());
            }
        }
        assert_eq!(
            key_only,
            vec!["is_sender".to_string()],
            "exactly one flag may be key-only; add it to every index's \
             `values`, or teach `flags_on` which of the two to key on"
        );
    }

    /// The membership rows the write path produces must carry every
    /// declared column — a column present in the spec and absent from
    /// the row is a row the composite indexes silently exclude.
    #[test]
    fn written_fields_cover_declared_columns() {
        let row = thread_row::ThreadRow {
            thread_id: "t1".into(),
            subject: String::new(),
            senders_csv: String::new(),
            count: 1,
            unread_count: 0,
            latest_date: 1,
            latest_preview: String::new(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
        };
        // The union of the two writers: the derived fields, plus the
        // per-user flags a fresh row is planted with. They are apart on
        // purpose — deriving the second group from the shared hash is
        // what let one owner's star reach another's row.
        let mut written: std::collections::BTreeSet<Vec<u8>> =
            thread_row::thread_user_pairs("u@x.com", &row)
                .into_iter()
                .map(|(k, _)| k)
                .collect();
        written.extend(
            thread_row::PER_USER_FLAGS
                .iter()
                .map(|f| f.as_bytes().to_vec()),
        );
        for (col, _) in &thread_user_spec().columns {
            assert!(
                written.contains(col),
                "declared column {} is never written",
                String::from_utf8_lossy(col)
            );
        }
    }
}
