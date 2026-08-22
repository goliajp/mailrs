//! The `threaduser` table's declaration, on its own so the file that owns
//! the declaration *lifecycle* stays readable — and under the 500-line
//! limit, which the two together no longer were.
//!
//! It is a function rather than a constant because `orderpath_columns_are_declared`
//! has to be able to call it: kevy panics rather than erroring when an
//! ORDERPATH names a column the table never declared, and that panic is on
//! the boot path.

use super::keys;

/// The `threaduser` table declaration.
///
/// Split out of `ensure_thread_table` so the column/orderpath
/// agreement can be asserted in a test: kevy panics rather than
/// returning an error when an ORDERPATH names a column the table
/// never declared, and that panic happens at boot.
pub(crate) fn thread_user_spec() -> kevy_embedded::TableSpec {
    use kevy_embedded::{IndexKind, IndexValType as ValType, OrderPath, TableIndex, TableSpec};

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
        "snoozed_until",
        // Stored beside every index as well as keyed by its own, so
        // "Starred, and only this mailbox" is one walk. Without it
        // here, the account could be keyed on but never filtered on,
        // and every stacked query would silently ignore it —
        // `is_sender_is_the_only_flag_that_must_be_the_key` is the
        // test that says so.
        "account_id",
    ]
    .iter()
    .map(|c| c.as_bytes().to_vec())
    .collect();

    TableSpec {
        name: b"threaduser".to_vec(),
        prefix: keys::THREAD_USER_PREFIX.to_vec(),
        pk: b"tid".to_vec(),
        // New in kevy-index 5.1, all three declared explicitly rather than
        // defaulted, because two of them change what this table is.
        //
        // `window`: no sliding hot window. Conversations do not age out of
        // relevance on a timer — a five-year-old thread is a legitimate
        // search hit — and a windowed table moves rows below the boundary
        // into cold segments, which 5.0 spent a release making correct.
        //
        // `autodeclare: 0`: the engine may not declare paths for us. The
        // whole point of this table is that a query axis is declared and
        // therefore reviewable (`kevy/declare-dont-maintain`); a path that
        // appeared because a query was refused once is a path nobody chose.
        // Off is also the upstream default.
        window: None,
        autodeclare: 0,
        // Runtime provenance, empty by construction here. It must never be
        // part of a "same declaration?" comparison — see `ensure_thread_table`.
        auto_added: Vec::new(),
        columns: vec![
            col("user", ValType::Str),
            col("tid", ValType::Str),
            col("ord", ValType::I64),
            col("bucket", ValType::Str),
            col("category", ValType::Str),
            col("activity", ValType::I64),
            // Which connected mailbox the conversation arrived at,
            // empty for this deployment's own. Stored and filtered on,
            // never keyed: putting it in an ORDERPATH would double
            // every composite for a predicate most people never use.
            col("account_id", ValType::Str),
            col("sent_only", ValType::I64),
            col("is_sender", ValType::I64),
            col("starred", ValType::I64),
            col("archived", ValType::I64),
            col("pinned", ValType::I64),
            col("unread", ValType::I64),
            col("has_action", ValType::I64),
            // Epoch seconds this person put the thread away until, or
            // 0. A stored value rather than an index of its own: the
            // read filters `snoozed_until <= now`, so a row comes back
            // when its time passes and nothing has to sweep it awake.
            // A flag would need that sweep, and a sweep that flips one
            // bit per thread per minute is the shape
            // `periodic-work-must-converge` exists to refuse.
            col("snoozed_until", ValType::I64),
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
            // Not a flag: keyed on the epoch second a thread is due
            // back. Every ordinary row stores 0, so asking for
            // `[1, now]` is an empty scan until something is actually
            // due — which is what makes waking cost nothing on the
            // calls where nothing has to happen.
            "snoozed_until",
            // Which connected mailbox the conversation arrived at.
            // Keyed like the flags rather than folded into an
            // ORDERPATH: "only these accounts" is N walks on this
            // index — the engine's value filters have `Eq` and no
            // `In` — and each carries `user` and `activity` beside it,
            // so one walk answers the page and its count the way the
            // flag axes already do.
            "account_id",
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
