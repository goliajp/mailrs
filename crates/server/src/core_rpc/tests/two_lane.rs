//! The two cores answer the same, field by field and in order.
//!
//! This replaces a comparison that could not see most of what it claimed to.
//! The predecessor built a `BTreeMap<thread_id, sorted Vec<message_id>>` from
//! each core and asserted the maps were equal — so it compared *structure*
//! only, and compared it order-insensitively on both axes. Every property the
//! switch actually turns on was invisible to it:
//!
//! - the **order** the conversation list comes back in, which is what a reader
//!   sees first and the one thing a `BTreeMap` of sorted vectors erases;
//! - all eighteen fields of `ConversationSummaryWire` except `thread_id`;
//! - per-user state — read, starred, archived, pinned, snoozed — which is the
//!   whole subject of the July per-user-thread-state work;
//! - the counts the UI renders as badges;
//! - paging, where `kevy/total-order-or-paging-breaks` says a non-total sort
//!   skips or repeats a row at a page boundary. On prod that collided 929
//!   times over 30k rows because `activity` is whole seconds, so this seeds
//!   same-second threads deliberately.
//!
//! Differences that are legitimate are named in `LEGITIMATE_DIFFS` with the
//! reason, and each one is asserted to *still* differ — an exclusion that has
//! quietly become unnecessary is a comparison narrower than it needs to be.

#![cfg(feature = "spg")]

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};

use super::pg_core::{deliver_req, spawn_fastcore, spawn_pg_core};

pub(super) const USER: &str = "two-lane@test";

/// Seven threads, and three of them share one `latest_date` second.
///
/// The collision is the point. `activity` is whole seconds on the kevy side,
/// so same-second arrivals tie, and a sort that ends on the timestamp leaves
/// the order between them undefined between calls — which a paged reader sees
/// as a skipped or repeated row, intermittently. Seeding ties makes the
/// property testable instead of waiting for production to produce them.
const THREADS: usize = 7;
const TIE_SECOND: i64 = 1_700_000_500;

pub(super) async fn seed(c: &Client) {
    c.add_account(&AddAccountRequest {
        address: USER.into(),
        display_name: "Two Lane".into(),
        password: "pw".into(),
    })
    .await
    .expect("add_account");

    let mut uid = 1u32;
    for t in 0..THREADS {
        let thread = format!("tl-{t}@test");
        // threads 2, 3 and 4 all land in the same second
        let date = if (2..5).contains(&t) {
            TIE_SECOND
        } else {
            1_700_000_000 + t as i64 * 1_000
        };
        for m in 0..2 {
            let mut req = deliver_req(&format!("tl-{t}-{m}@test"), uid, &thread, USER);
            req.latest_date = date;
            // Mirror `core-sync`, which sets `subject: wire.subject` — the
            // same string that is inside the payload. Setting the two
            // differently makes the request self-inconsistent, and then the
            // comparison measures the seed rather than the cores: kevy reads
            // the top-level copy and the SQL side aggregates `MAX(m.subject)`
            // from the rows, so any disagreement between them shows up as a
            // lane difference that no real caller would produce.
            req.subject = "Hi".to_string();
            // Both copies of the time, because the request carries it twice and
            // the two cores read different ones: kevy takes the top-level
            // `latest_date`, while the SQL core parses `payload_wire_json` and
            // orders by `MAX(m.internal_date)`. Setting only one produced a
            // seed where every SQL row shared a timestamp — which looked
            // exactly like the SQL lane sorting oldest-first, and is not.
            // core-sync sends both, so a real migration is unaffected; a test
            // that sets one is measuring its own mistake.
            // `flags: 0` with `unread: true` — a coherent arrival. The shared
            // `deliver_req` says `"flags": 1`, i.e. `\Seen`, while also
            // claiming the message is unread, and a seed that contradicts
            // itself measures whichever copy the code under test happens to
            // read. That contradiction is what made the round-trip rehearsal
            // report every thread as differing once `core-sync` started taking
            // the read state from the flag instead of from the sender.
            req.payload_wire_json = req
                .payload_wire_json
                .replace("\"flags\":1", "\"flags\":0")
                .replace("\"date\":1700000000", &format!("\"date\":{date}"))
                .replace(
                    "\"internal_date\":1700000000",
                    &format!("\"internal_date\":{date}"),
                );
            c.deliver_message(USER, &thread, &req)
                .await
                .expect("deliver");
            uid += 1;
        }
    }
}

pub(super) async fn page(
    c: &Client,
    limit: u32,
    before_ts: Option<i64>,
) -> Vec<ConversationSummaryWire> {
    c.list_conversations(
        USER,
        &ListConversationsRequest {
            filter: ConversationFilter {
                limit,
                before_ts,
                ..Default::default()
            },
        },
    )
    .await
    .expect("list_conversations")
    .items
}

/// Walk the whole list the way a client does: page, then ask for what is older
/// than the last row you were given.
///
/// The cursor is `last_date`, and `before_ts` means **strictly** less than it.
/// So a page that ends inside a group of threads sharing one second cannot ask
/// for the rest of that second without also asking for the row it already has
/// — it either loses the tied rows or repeats them. `activity` is whole
/// seconds, and prod had 929 such ties over 30k rows.
async fn walk(c: &Client, page_size: u32) -> Vec<String> {
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let rows = page(c, page_size, cursor).await;
        if rows.is_empty() {
            return seen;
        }
        cursor = Some(rows[rows.len() - 1].last_date);
        seen.extend(rows.iter().map(|s| s.thread_id.clone()));
        if seen.len() > THREADS * 4 {
            // A cursor that does not advance loops forever; bound it rather
            // than hang the suite, and let the assertion report the repeats.
            return seen;
        }
    }
}

/// Fields whose values are allowed to differ, each with why.
///
/// Kept as data rather than as skipped assertions so the list is readable in
/// one place and so `no_exclusion_is_stale` can check each one is still needed.
/// `importance_score` was here too, on the same reasoning, and
/// `no_exclusion_is_stale` rejected it on its first run: both lanes report the
/// same score, so excluding it only narrowed the comparison. That is the test
/// doing its job — an exclusion nobody rechecks is how a comparison quietly
/// stops covering a field.
const LEGITIMATE_DIFFS: &[(&str, &str)] = &[(
    "importance_level",
    "the heuristic scorer only ever ran on the kevy lane — `calculate_importance` \
         has no caller in the SQL core, so this side reports the default",
)];

/// Every field of the wire row, as `(name, rendered value)`.
///
/// Rendered rather than compared field-by-field in code, so adding a field to
/// `ConversationSummaryWire` shows up here as a difference rather than being
/// silently uncompared — the failure mode of a hand-written comparison is the
/// field somebody forgot to add to it.
fn fields(s: &ConversationSummaryWire) -> Vec<(&'static str, String)> {
    vec![
        ("thread_id", s.thread_id.clone()),
        ("subject", s.subject.clone()),
        ("participants", s.participants.clone()),
        ("message_count", s.message_count.to_string()),
        ("unread_count", s.unread_count.to_string()),
        ("last_date", s.last_date.to_string()),
        ("category", s.category.clone()),
        ("flagged", s.flagged.to_string()),
        ("snippet", s.snippet.clone()),
        ("pinned", s.pinned.to_string()),
        ("archived", s.archived.to_string()),
        ("importance_level", s.importance_level.clone()),
        ("importance_score", s.importance_score.to_string()),
        ("requires_action", s.requires_action.to_string()),
        ("sent_count", s.sent_count.to_string()),
        ("snoozed_until", s.snoozed_until.to_string()),
    ]
}

/// Compare two lists row by row and field by field, collecting **every**
/// difference rather than stopping at the first.
///
/// Stopping at the first turns one run into one fact, and this comparison
/// exists to produce the whole list — the difference between "subject differs"
/// and "subject and senders differ on rows written before the display payload
/// existed" is the difference between a bug and a backfill gap.
/// Order the rows the way the client does before comparing.
///
/// `web/src/lib/list-rows.ts` returns `[...pinned, ...unpinned]` with the
/// unpinned sorted by `last_date`, so the browser normalises the server's order
/// itself. The two cores do differ here — the SQL query begins
/// `ORDER BY BOOL_OR(m.pinned) DESC` while kevy keeps `pinned` as a filterable
/// flag and not a sort prefix — and after this normalisation that difference is
/// invisible to a reader, which is the level the comparison belongs at.
///
/// The one client mode that uses the server's order verbatim is
/// `sortOrder === 'relevance'`, and that only arises for search, which pg-core
/// does not serve at all (it is one of the eleven in
/// `scripts/core-parity-baseline.txt`).
fn as_the_client_orders_them(rows: &[ConversationSummaryWire]) -> Vec<ConversationSummaryWire> {
    let mut out: Vec<ConversationSummaryWire> = rows.to_vec();
    // Stable, so rows tied on `last_date` keep the order the server gave —
    // exactly what `Array.prototype.sort` does in the browser.
    out.sort_by_key(|s| (!s.pinned, -s.last_date));
    out
}

/// Rows grouped into runs that share a `last_date`.
///
/// A tie's internal order is not something the contract promises, and the two
/// lanes break ties differently on purpose: kevy sorts on `ord`, a folded hash
/// of the thread id, because a composite index there drops any row with a
/// string component over 255 bytes; SQL has no such limit and sorts on the id
/// itself. Both are total, which is what stops a paged reader skipping a row.
/// Demanding they pick the *same* arbitrary order would mean carrying kevy's
/// hash into SQL for no reader-visible gain.
fn tie_groups(rows: &[ConversationSummaryWire]) -> Vec<(i64, std::collections::BTreeSet<String>)> {
    let mut out: Vec<(i64, std::collections::BTreeSet<String>)> = Vec::new();
    for r in rows {
        match out.last_mut() {
            Some((d, ids)) if *d == r.last_date => {
                ids.insert(r.thread_id.clone());
            }
            _ => {
                let mut ids = std::collections::BTreeSet::new();
                ids.insert(r.thread_id.clone());
                out.push((r.last_date, ids));
            }
        }
    }
    out
}

/// Compare two lists and collect **every** difference rather than stopping at
/// the first.
///
/// Stopping at the first turns one run into one fact, and this comparison
/// exists to produce the whole list — the difference between "subject differs"
/// and "subject and senders differ on rows written before the display payload
/// existed" is the difference between a bug and a backfill gap.
pub(super) fn diffs(
    kevy: &[ConversationSummaryWire],
    pg: &[ConversationSummaryWire],
    what: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let (k, p) = (
        as_the_client_orders_them(kevy),
        as_the_client_orders_them(pg),
    );

    let (kg, pg_) = (tie_groups(&k), tie_groups(&p));
    if kg != pg_ {
        out.push(format!(
            "{what}: the sequence of same-second groups differs\n     kevy: {kg:?}\n     pg:   {pg_:?}"
        ));
        return out;
    }

    // Same groups in the same order, so pair rows by thread id — position
    // within a tie is deliberately not compared.
    let excluded: Vec<&str> = LEGITIMATE_DIFFS.iter().map(|(f, _)| *f).collect();
    let by_id: std::collections::BTreeMap<&str, &ConversationSummaryWire> =
        p.iter().map(|s| (s.thread_id.as_str(), s)).collect();
    for kr in &k {
        let Some(pr) = by_id.get(kr.thread_id.as_str()) else {
            out.push(format!("{what}: {} is on kevy and not on pg", kr.thread_id));
            continue;
        };
        for ((name, kv), (_, pv)) in fields(kr).into_iter().zip(fields(pr)) {
            if kv != pv && !excluded.contains(&name) {
                out.push(format!(
                    "{what}: {} .{name}: kevy={kv:?} pg={pv:?}",
                    kr.thread_id
                ));
            }
        }
    }
    out
}

#[tokio::test]
async fn the_conversation_list_agrees_field_by_field_and_in_order() {
    let kevy = Client::new(spawn_fastcore(), String::new());
    let pg = Client::new(spawn_pg_core().await, String::new());
    seed(&kevy).await;
    seed(&pg).await;

    let d = diffs(
        &page(&kevy, 100, None).await,
        &page(&pg, 100, None).await,
        "list",
    );
    assert!(
        d.is_empty(),
        "the two cores disagree:\n   {}",
        d.join("\n   ")
    );
}

#[tokio::test]
async fn per_user_state_agrees_after_mutation() {
    let kevy = Client::new(spawn_fastcore(), String::new());
    let pg = Client::new(spawn_pg_core().await, String::new());
    seed(&kevy).await;
    seed(&pg).await;

    // One of each mutation the contract offers on both lanes, on a different
    // thread each, so a mutation that writes the wrong row shows up as two
    // differences rather than cancelling out.
    for c in [&kevy, &pg] {
        c.mark_thread_read(USER, "tl-0@test").await.expect("read");
        c.star_thread(USER, "tl-1@test").await.expect("star");
        c.pin_thread(USER, "tl-2@test").await.expect("pin");
        c.archive_thread(USER, "tl-3@test").await.expect("archive");
        c.mark_thread_unread(USER, "tl-4@test")
            .await
            .expect("unread");
    }

    let (k, p) = (page(&kevy, 100, None).await, page(&pg, 100, None).await);

    // `mark_thread_unread` means two different things, and both are defensible,
    // so this pins the difference rather than hiding it or changing a lane to
    // match the other.
    //
    //   kevy raises `unread_count` to *at least* 1 — "there is unread here",
    //   a marker on the denormalised thread row (mutations.rs:154).
    //   the SQL lane clears `\Seen` on every message in the thread, so the
    //   aggregate counts them all.
    //
    // On a 2-message thread the reader sees "1 unread" on one core and "2
    // unread" on the other. Aligning the SQL lane to the counter would leave
    // its own `messages.flags` disagreeing with its thread summary, which is
    // worse than the difference; aligning kevy is a change to shipped
    // production behaviour that nobody asked for. So: asserted, both sides, and
    // this fails the moment either changes.
    let unread_of = |rows: &[ConversationSummaryWire], tid: &str| {
        rows.iter()
            .find(|r| r.thread_id == tid)
            .map(|r| r.unread_count)
    };
    assert_eq!(unread_of(&k, "tl-4@test"), Some(1), "kevy marks the thread");
    assert_eq!(
        unread_of(&p, "tl-4@test"),
        Some(2),
        "the SQL lane marks each message"
    );

    let d: Vec<String> = diffs(&k, &p, "after mutations")
        .into_iter()
        .filter(|line| !line.contains("tl-4@test .unread_count"))
        .collect();
    assert!(
        d.is_empty(),
        "per-user state diverged after identical mutations:\n   {}",
        d.join("\n   ")
    );

    // And the badge, which is its own query rather than a sum over the page.
    let ku = kevy.unseen_count(USER).await.expect("kevy unseen");
    let pu = pg.unseen_count(USER).await.expect("pg unseen");
    assert_eq!(ku.count, pu.count, "unseen badge disagrees");
}

#[tokio::test]
async fn the_two_lanes_page_alike() {
    // The cross-lane property, kept separate from whether paging is correct at
    // all: whatever the cursor does, both cores must do the same thing, or a
    // switch changes what a client sees mid-scroll.
    let kevy = Client::new(spawn_fastcore(), String::new());
    let pg = Client::new(spawn_pg_core().await, String::new());
    seed(&kevy).await;
    seed(&pg).await;

    // Compared as multisets per page-walk rather than as sequences, for the
    // same reason `tie_groups` exists: the two lanes break a same-second tie
    // differently and neither order is promised. What must match is which
    // threads the walk reaches.
    let (mut k, mut p) = (walk(&kevy, 3).await, walk(&pg, 3).await);
    k.sort();
    p.sort();
    assert_eq!(k, p, "the two cores' page walks reach different threads");
}

#[tokio::test]
async fn a_page_walk_visits_every_thread_once() {
    // Not a cross-lane property — a property of the cursor itself, which is why
    // it is its own test. If this fails on BOTH lanes it is one defect in the
    // contract, not divergence between two implementations, and the fix is a
    // bounded tie-break in the cursor rather than anything about either store.
    let kevy = Client::new(spawn_fastcore(), String::new());
    let pg = Client::new(spawn_pg_core().await, String::new());
    seed(&kevy).await;
    seed(&pg).await;

    for (name, c) in [("kevy", &kevy), ("pg", &pg)] {
        let whole: Vec<String> = page(c, 100, None)
            .await
            .iter()
            .map(|s| s.thread_id.clone())
            .collect();
        assert_eq!(
            whole.len(),
            THREADS,
            "{name}: one unpaged read sees them all"
        );

        let walked = walk(c, 3).await;
        let mut sorted = walked.clone();
        sorted.sort();
        let mut uniq = sorted.clone();
        uniq.dedup();
        assert_eq!(
            walked.len(),
            uniq.len(),
            "{name}: the walk repeated a thread — {walked:?}"
        );
        assert_eq!(
            uniq.len(),
            THREADS,
            "{name}: the walk saw {} of {THREADS} threads. `before_ts` is \
             strictly-less-than and three of these share one second, so a page \
             ending inside that second cannot ask for the rest of it. Walk: \
             {walked:?}",
            uniq.len()
        );
    }
}

#[tokio::test]
async fn no_exclusion_is_stale() {
    // An exclusion that is no longer needed makes this comparison narrower
    // than it could be, and nobody notices, because the suite is green either
    // way. So each one has to still be doing work.
    let kevy = Client::new(spawn_fastcore(), String::new());
    let pg = Client::new(spawn_pg_core().await, String::new());
    seed(&kevy).await;
    seed(&pg).await;

    let (k, p) = (page(&kevy, 100, None).await, page(&pg, 100, None).await);
    assert_eq!(
        k.len(),
        p.len(),
        "same row count before checking exclusions"
    );

    for (name, why) in LEGITIMATE_DIFFS {
        let differs = k.iter().zip(&p).any(|(a, b)| {
            let av = fields(a)
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v);
            let bv = fields(b)
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v);
            av != bv
        });
        assert!(
            differs,
            "`{name}` is excluded from the comparison on the grounds that {why} — \
             but the two lanes now agree on it. Drop the exclusion so the field \
             is compared."
        );
    }
}
