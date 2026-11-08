//! The sectioned inbox in SQL: the same rules as
//! [`crate::section::section_of`], over the facts delivery recorded on each
//! message. The two are held to the same answers by the core contract test.
//!
//! Only messages with recorded facts count (`direct IS NOT NULL`); `invite`
//! and `mutual`, recorded later than the rest, read as false where NULL. A
//! conversation none of whose messages has them is undecided (`''`), as a
//! kevy row no writer has reached is. Ties at one instant go to the later
//! row, as the kevy writer lets the later arrival win.

/// Aggregates over one user's messages in a thread, appended to the row's
/// select list. `mb.name = 'Sent'` is the user's own mail in this store.
pub(super) const AGGREGATES: &str = ",
                BOOL_OR(m.direct IS NOT NULL) AS s_recorded,
                COALESCE(MAX(CASE WHEN mb.name = 'Sent' AND m.direct IS NOT NULL THEN m.internal_date END), 0) AS s_own_at,
                COALESCE((array_agg(m.to_others ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name = 'Sent' AND m.direct IS NOT NULL))[1], false) AS s_own_to_others,
                COALESCE(MAX(CASE WHEN mb.name != 'Sent' AND m.from_person THEN m.internal_date END), 0) AS s_person_at,
                COALESCE(MAX(CASE WHEN mb.name != 'Sent' AND m.direct IS NOT NULL THEN m.internal_date END), 0) AS s_in_at,
                COALESCE((array_agg(m.direct ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name != 'Sent' AND m.direct IS NOT NULL))[1], false) AS s_in_direct,
                COALESCE((array_agg(m.from_person ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name != 'Sent' AND m.direct IS NOT NULL))[1], false) AS s_in_person,
                COALESCE((array_agg(m.first_contact ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name != 'Sent' AND m.direct IS NOT NULL))[1], false) AS s_in_first,
                COALESCE((array_agg(m.from_addr ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name != 'Sent' AND m.direct IS NOT NULL))[1], '') AS s_in_from,
                COALESCE((array_agg(COALESCE(m.invite, false) ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name != 'Sent' AND m.direct IS NOT NULL))[1], false) AS s_in_invite,
                COALESCE((array_agg(COALESCE(m.mutual, false) ORDER BY m.internal_date DESC, m.id DESC)
                          FILTER (WHERE mb.name != 'Sent' AND m.direct IS NOT NULL))[1], false) AS s_in_mutual";

/// The section of row `v`, `$1` the user. `bucket` is the row's bucket as
/// SQL, read off `v.category`.
pub(super) fn section_case(bucket: &str) -> String {
    format!(
        "CASE
           WHEN NOT v.s_recorded THEN ''
           WHEN {bucket} = 'junk' THEN ''
           WHEN v.s_own_at > 0 AND v.s_own_at > v.s_person_at AND v.s_own_to_others THEN 'awaiting'
           WHEN v.s_in_at = 0 THEN 'other'
           WHEN NOT v.s_in_person THEN 'feed'
           WHEN v.s_in_first AND v.s_own_at = 0 AND NOT EXISTS (
                SELECT 1 FROM accepted_senders acc
                 WHERE acc.account_address = $1 AND acc.address = v.s_in_from) THEN 'new'
           WHEN v.s_in_direct AND NOT v.s_in_invite AND (v.s_own_at > 0 OR v.s_in_mutual) THEN 'reply'
           ELSE 'other'
         END"
    )
}

/// Wrap the per-thread select so its rows carry `section` and
/// `awaiting_since`, under the alias the filters use (`t`).
pub(super) fn with_sections(inner: &str, bucket_of_v: &str) -> String {
    format!(
        "SELECT u.*, CASE WHEN u.section = 'awaiting' THEN u.s_own_at ELSE 0 END AS awaiting_since
           FROM (SELECT v.*, {} AS section FROM ({inner}) v) u",
        section_case(bucket_of_v)
    )
}
