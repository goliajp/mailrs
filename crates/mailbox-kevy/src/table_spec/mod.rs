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

mod spec;
pub(crate) use spec::thread_user_spec;

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
    /// NOTE: `kevy-index` stays a direct dependency, pinned to the same
    /// version as `kevy-embedded` so the two cannot end up a generation
    /// apart in one binary.
    ///
    /// The reason has changed twice and the old note was wrong on both
    /// counts. `TableSpec` / `TableIndex` / `OrderPath` have been
    /// re-exported since 4.1.1, and those now come through the facade.
    /// What still needs the direct dependency is `WhereClause`,
    /// `CompositeCol` and `composite_bounds` — this crate computes an
    /// ORDERPATH's composite bounds itself (see `table_query/`), because
    /// the facade offers no typed way to ask for an equality prefix plus
    /// one range. Re-checked against 5.1: still the case.
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
            // `sans_auto()`, not `==`: `auto_added` is runtime provenance, and
            // comparing it would read every engine-declared path as drift —
            // dropping and rebuilding the whole table on each boot. Upstream
            // says ENSURE-style comparisons go through this method, and with
            // `autodeclare: 0` there is nothing there today; the form is right
            // regardless, and the failure it prevents is a boot loop.
            Some(ref existing) if existing.sans_auto() == spec => {
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
                .hgetall(&key)?
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
            self.store.hset(&key, &zeros)?;
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
            snoozed_until: 0,
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
