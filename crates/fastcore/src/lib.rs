//! `mailrs-fastcore` — Kevy-backed implementation of the
//! `mailrs-core-api` server surface. Phase 8.
//!
//! Today this binary mounts a small subset:
//! - `/v1/healthz` + `/v1/readyz` (open) — proves the role works
//! - `POST /v1/users/{user}/conversations:list` — Rock 1 read path
//!
//! The rest of the 87-route surface fills in as `mailbox-kevy` grows
//! method coverage. Run alongside (or instead of) the monolith core
//! to A/B test conversation-list latency under the same load.
//!
//! Environment:
//! - `MAILRS_FASTCORE_BIND` — listen address (default `0.0.0.0:3301`,
//!   one above the monolith's core-rpc :3300 so both can coexist)
//! - `MAILRS_KEVY_DATA_DIR` — kevy persist dir (default
//!   `/data/kevy-fastcore`)

#![allow(missing_docs)]

mod acme_task;
mod aof_compact;
pub mod arc_seal;
mod backfill_decode;
mod bayes_train;
pub mod boot;
pub use boot::run;

pub mod bounce;
mod calendar_sync;
pub mod dmarc_ingest;
pub mod external_sync;
mod external_sync_jmap;
mod external_sync_pop3;
pub mod external_sync_secret;
pub mod fbl;
mod headers;
mod idle_backoff;
mod imap;
mod importance;
mod ingest;
pub mod invites;
mod junk_ttl;
mod keywords;
pub mod live_sync;
mod maildir_scan;
mod maintenance;
mod managesieve;
mod pop3;
mod push;
mod router;
mod routes;
mod snooze_wake;
mod store_motion;
pub use store_motion::store_motion_probe;
mod threadstate;
mod uidlist;
use headers::*;
use ingest::*;
use maildir_scan::*;
use maintenance::*;
pub use router::build_router;

/// Run one full self-heal sweep for one user.
///
/// A seam for the integration tests, which drive the real sweep rather
/// than its four phases: the thing worth pinning about a rebuild is what
/// the whole of it produces, and the phases share state through the store.
pub async fn self_heal_once(state: &std::sync::Arc<FastcoreState>, user: &str) -> bool {
    maildir_scan::healed_from_maildir(state, user, 0).await
}
use routes::*;
pub mod sender_sts;
mod sieve_apply;
mod spool_drain;
pub mod tlsrpt;
pub mod tlsrpt_ingest;
mod webhook_delivery;

use kevy_embedded::{Config, Store};
use mailrs_alias_store::AliasStore;
use mailrs_core_api::method::admin as adm;
use mailrs_core_api::method::analysis as an;
use mailrs_core_api::method::contact as ct;
use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::mailbox as mb;
use mailrs_core_api::method::message as msg;
use mailrs_core_api::method::outbound as ob;
use mailrs_core_api::method::thread as th;
use mailrs_core_api::server::{Handler, base_router};
use mailrs_core_api::types::{BackendKind, ConversationSummaryWire, HealthResponse};
use mailrs_mailbox_kevy::{KevyMailboxStore, ListThreadsFilter, ThreadRow};

/// Server state — owns the kevy store and is cloned into axum handlers.
pub struct FastcoreState {
    pub mailbox: KevyMailboxStore,
    /// Alias resolver / admin. Backend-agnostic: fastcore's boot code
    /// currently constructs an `Arc<KevyMailboxStore>` here (embedded
    /// kevy), but any [`AliasStore`] impl works — the planned
    /// network-kevy backend (RFC 20260705) drops in without touching
    /// call sites. Handlers hold `state.clone()`, so `Arc` is required.
    pub alias_store: std::sync::Arc<dyn AliasStore>,
    /// In-process delivery fanout: every write path publishes the
    /// recipient address here; IMAP IDLE sessions subscribe and push
    /// `* n EXISTS` to their client (RFC 2177). Drain + RPC + IMAP all
    /// live in this process, so no kevy pub/sub hop is needed.
    pub notify: tokio::sync::broadcast::Sender<String>,
    /// False when the store's boot report showed a damaged AOF.
    ///
    /// kevy 4.0 turns the boot verdict into data (`Store::open_report`)
    /// rather than a line on stderr. That exists because of our own
    /// incident: a corrupt frame black-holed three days of writes while
    /// every restart looked normal. Surfacing it here means a deploy
    /// over a damaged boot cannot go green — the container keeps
    /// serving mail (a live-but-unhealthy instance still delivers; a
    /// dead one does not) but the health check refuses.
    pub boot_intact: bool,
    /// Network-kevy URL (`MAILRS_KEVY_URL`) for the shared side-state
    /// routes (drafts / signatures / templates / reactions / webhooks /
    /// audit / outbound / groups). These live in the INDEPENDENT network
    /// kevy — the same keys webapi + the pg-core read — so both cores
    /// serve them identically. `None` in tests / when unset: side-state
    /// routes return empty results rather than erroring.
    pub net_url: Option<String>,
}

impl FastcoreState {
    /// Construct state with a fresh notify channel. Reads the network-kevy
    /// URL from `MAILRS_KEVY_URL` (absent in tests → side-state disabled).
    /// Alias store defaults to the embedded-kevy backend backed by the
    /// same `mailbox` handle; swap in a network-kevy impl at the boot
    /// site when RFC 20260705 Step 2 lands.
    pub fn new(mailbox: KevyMailboxStore) -> Self {
        let alias_store: std::sync::Arc<dyn AliasStore> = std::sync::Arc::new(mailbox.clone());
        Self::new_with_alias_store(mailbox, alias_store)
    }

    /// Construct with an explicit alias-store backend. Used by tests and
    /// by the planned network-kevy boot path; the default constructor
    /// wires the embedded-kevy impl for backwards compatibility.
    pub fn new_with_alias_store(
        mailbox: KevyMailboxStore,
        alias_store: std::sync::Arc<dyn AliasStore>,
    ) -> Self {
        let (notify, _) = tokio::sync::broadcast::channel(256);
        let net_url = std::env::var("MAILRS_KEVY_URL")
            .ok()
            .filter(|s| !s.is_empty());
        Self {
            mailbox,
            alias_store,
            notify,
            net_url,
            boot_intact: true,
        }
    }

    /// Record the store's boot verdict. Called once at startup with the
    /// result of `Store::open_report`; see [`Self::boot_intact`].
    pub fn with_boot_intact(mut self, intact: bool) -> Self {
        self.boot_intact = intact;
        self
    }

    /// Open a fresh network-kevy connection for a side-state handler.
    /// Follows the per-use `Connection::open` pattern the auxiliary tasks
    /// use (spool_drain / live_sync / sieve_apply). Returns `None` when no
    /// network kevy is configured so handlers can serve an empty result.
    pub fn net_conn(&self) -> Option<kevy_client::Connection> {
        let url = self.net_url.as_ref()?;
        kevy_client::Connection::connect(url).ok()
    }
}

impl mailrs_core_sidestate::NetKevy for FastcoreState {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        FastcoreState::net_conn(self)
    }
}

impl Handler for FastcoreState {
    async fn healthz(&self) -> HealthResponse {
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: self.boot_intact,
        }
    }

    async fn readyz(&self) -> HealthResponse {
        // kevy is in-process, so the store is up whenever the binary
        // is — but "up" is not "intact". A boot that dropped bytes is
        // serving a keyspace smaller than the files held, and that must
        // not read as ready.
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Kevy,
            ready: self.boot_intact,
        }
    }
}

fn strip_angle(v: &str) -> String {
    let t = v.trim();
    if let Some(inner) = t.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        inner.trim().to_string()
    } else {
        t.trim_matches(|c: char| c == '<' || c == '>').to_string()
    }
}

/// Very small RFC 5322 date parser: `Wed, 01 Jul 2026 12:34:56 +0000`.
/// Only accepts `+0000`/`-0000`-style offsets; that covers everything
/// modern MTAs emit. Full parse coverage lives on `time` crate; we
/// don't need to pull it in for the fallback.
/// Parse an RFC 5322 `Date:` header value to unix epoch seconds (UTC).
///
/// Delegates to `chrono::DateTime::parse_from_rfc2822`, which handles
/// every real-world variant we see: `Sat, 13 Jun 2026 06:01:22 +0000`,
/// `Fri, 3 Jul 2026 02:40:42 +0900` (Gmail), `13 Jun 2026 06:01:22 GMT`
/// (no day-of-week), and named zones (`GMT`/`UTC`/`EST`/…). Timezones
/// are correctly normalised to UTC before the epoch conversion — the
/// previous hand-rolled parser dropped the zone entirely, so an email
/// stamped in JST landed nine hours off and inbound replies could sort
/// ahead of the sent copy.
///
/// Returns `None` when the header is empty / unparseable.
fn parse_rfc5322_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    // Retry ladder for the messy real world:
    //   1. Strip a trailing " (CFWS)" comment (RFC 5322 §3.3 permits it,
    //      chrono rejects it).
    //   2. Strip a leading "Weekday, " prefix — many senders ship a
    //      day-of-week that disagrees with the date (chrono treats that
    //      as Impossible even though the timestamp is well-formed).
    let no_comment = s.split(" (").next().unwrap_or(s).trim_end();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(no_comment) {
        return Some(dt.timestamp());
    }
    let no_dow = match no_comment.find(", ") {
        Some(idx) => no_comment[idx + 2..].trim_start(),
        None => no_comment,
    };
    chrono::DateTime::parse_from_rfc2822(no_dow)
        .ok()
        .map(|dt| dt.timestamp())
}

// ── Account (auth) — Phase 8 ────────────────────────────────────────

// ── Mailboxes (folders) ────────────────────────────────────────────

// ── Thread mutations ───────────────────────────────────────────────

use axum::response::IntoResponse;

// ── Group B: admin write handlers ─────────────────────────────────
//
// The webapi used to write account / permission / message blobs to
// the network kevy directly (`MAILRS_KEVY_URL`). Fastcore reads its
// own embedded kevy at `/data/kevy-fastcore`, so those writes never
// affected login / account list / update_flags. These handlers close
// the gap: webapi calls fastcore RPCs, fastcore mutates its embedded
// kevy through the same `KevyMailboxStore` used at boot / ingest.

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod spoof_landing_tests {
    use super::from_header_domains;

    /// The domain decides whether a DMARC failure is somebody forging one
    /// of our users or just a stranger with a broken setup, so reading it
    /// off a `From:` line is the whole judgement this route makes.
    #[test]
    fn it_reads_the_domain_out_of_the_ordinary_forms() {
        assert_eq!(
            from_header_domains("From: Netflix <takagi@golia.jp>"),
            ["golia.jp"]
        );
        assert_eq!(from_header_domains("From: takagi@golia.jp"), ["golia.jp"]);
        assert_eq!(from_header_domains("from: A B <x@GOLIA.JP>"), ["golia.jp"]);
    }

    /// A display name may itself contain an `@`, which is exactly what a
    /// sender trying to look like one of ours would put there. Reading the
    /// first `@` takes the domain from the quoted part and concludes the
    /// message forged nothing.
    #[test]
    fn a_display_name_containing_an_at_does_not_win() {
        assert_eq!(
            from_header_domains("From: \"billing@paypal.com\" <attacker@evil.example>"),
            ["evil.example"]
        );
    }

    /// The case that caught the first version: it read the *last* `@` and
    /// answered `other.com`, so a header claiming one of ours alongside a
    /// stranger counted as not ours at all. Both are claimed, so both are
    /// returned and the caller checks for any hosted one.
    #[test]
    fn every_address_in_the_header_is_returned() {
        assert_eq!(
            from_header_domains("From: a@golia.jp, b@other.com"),
            ["golia.jp", "other.com"]
        );
        assert_eq!(
            from_header_domains("From: A <a@other.com>, B <b@golia.jp>"),
            ["other.com", "golia.jp"]
        );
    }

    /// A trailing dot is the same domain (RFC 1034 root form).
    #[test]
    fn it_drops_the_root_dot_and_trailing_noise() {
        assert_eq!(from_header_domains("From: <a@golia.jp.>"), ["golia.jp"]);
        assert_eq!(from_header_domains("From: a@golia.jp\r"), ["golia.jp"]);
    }

    #[test]
    fn a_header_with_no_usable_domain_yields_nothing() {
        assert!(from_header_domains("From: not-an-address").is_empty());
        assert!(from_header_domains("From: a@").is_empty());
        assert!(from_header_domains("From:").is_empty());
    }
}

#[cfg(test)]
mod tests {

    /// A message that has been read lives in `cur/` with a `:2,FLAGS`
    /// suffix, and `read_maildir_file` used to reconstruct the filename by
    /// hand and miss it. That made the threading backfill's References
    /// edges invisible for every sent copy — `mirror_send` marks those Seen
    /// — so conversations that should have merged did not (2026-07-30).
    #[test]
    fn read_maildir_file_finds_a_flagged_message() {
        let tmp = std::env::temp_dir().join(format!("mailrs-rmf-{}", std::process::id()));
        let box_dir = tmp.join("x.com").join("bob");
        std::fs::create_dir_all(box_dir.join("cur")).unwrap();
        std::fs::create_dir_all(box_dir.join("new")).unwrap();

        // Unflagged, still in new/ — the case that already worked.
        std::fs::write(box_dir.join("new").join("plain.id"), b"raw-new").unwrap();
        // Read, so renamed into cur/ with a flag suffix.
        std::fs::write(box_dir.join("cur").join("seen.id:2,S"), b"raw-seen").unwrap();

        // SAFETY-adjacent: the env var is read inside the function under
        // test, and this is the only test that sets it.
        unsafe { std::env::set_var("MAILRS_MAILDIR", &tmp) };

        assert_eq!(
            read_maildir_file("bob@x.com", "plain.id").as_deref(),
            Some(&b"raw-new"[..]),
        );
        assert_eq!(
            read_maildir_file("bob@x.com", "seen.id").as_deref(),
            Some(&b"raw-seen"[..]),
            "a flagged file must be found by its base id"
        );
        assert!(read_maildir_file("bob@x.com", "absent.id").is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }
    use std::sync::Arc;

    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use mailrs_mailbox_kevy::MessageArrival;
    use tower::ServiceExt;

    pub(super) fn fresh_state() -> Arc<FastcoreState> {
        let store = Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("in-memory kevy"),
        );
        let mailbox = KevyMailboxStore::new(store);
        Arc::new(FastcoreState::new(mailbox))
    }

    pub(super) fn arr<'a>(tid: &'a str, user: &'a str, unread: bool) -> MessageArrival<'a> {
        MessageArrival {
            thread_id: tid,
            user,
            subject: "Subj",
            senders_csv: "x@y.z",
            latest_date: 100,
            latest_preview: "preview",
            category: "inbox",
            unread,
            is_own: !unread,
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn sweep_legacy_admin_keys_clears_legacy_and_keeps_v2() {
        let state = fresh_state();
        let store = state.mailbox.store_ref();
        // Seed the pre-P6 legacy layout + a v2 hash that must survive.
        store.set(b"mailrs:alias:old@x", b"target@x").unwrap();
        store
            .set(b"mailrs:domain:old.example", b"1700000000")
            .unwrap();
        store
            .sadd(b"mailrs:aliases:index", &[b"old@x".as_slice()])
            .unwrap();
        store
            .sadd(b"mailrs:domains:index", &[b"old.example".as_slice()])
            .unwrap();
        store
            .sadd(b"mailrs:accounts:index", &[b"a@x".as_slice()])
            .unwrap();
        state.mailbox.upsert_alias("keep@x", "target@x").unwrap();

        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/admin/maintenance:sweep-legacy-admin-keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        assert!(body.contains("\"legacy_alias_strings\":1"), "{body}");
        assert!(body.contains("\"legacy_domain_strings\":1"), "{body}");
        assert!(body.contains("\"legacy_index_sets\":3"), "{body}");

        // Legacy keys gone; v2 hash intact.
        assert!(store.get(b"mailrs:alias:old@x").unwrap().is_none());
        assert!(store.get(b"mailrs:domain:old.example").unwrap().is_none());
        assert!(store.smembers(b"mailrs:aliases:index").unwrap().is_empty());
        assert_eq!(
            state.mailbox.resolve_alias("keep@x").unwrap().as_deref(),
            Some("target@x")
        );

        // Idempotent: second sweep finds nothing.
        let app2 = build_router(state);
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/admin/maintenance:sweep-legacy-admin-keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body2 = body_string(resp2).await;
        assert!(body2.contains("\"legacy_alias_strings\":0"), "{body2}");
        assert!(body2.contains("\"legacy_index_sets\":0"), "{body2}");
    }

    #[tokio::test]
    async fn healthz_reports_kevy_backend() {
        let app = build_router(fresh_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        assert!(body.contains("\"backend\":\"kevy\""), "{body}");
    }

    #[tokio::test]
    async fn unseen_count_after_arrival_is_one() {
        let state = fresh_state();
        state
            .mailbox
            .record_message_arrival(&arr("t1", "u@x.com", true))
            .unwrap();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/users/u@x.com/conversations/unseen-count")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(body_string(resp).await.contains("\"count\":1"));
    }

    #[tokio::test]
    async fn mark_read_drops_from_unseen() {
        let state = fresh_state();
        state
            .mailbox
            .record_message_arrival(&arr("t1", "u@x.com", true))
            .unwrap();
        let app = build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/threads/t1/read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            state
                .mailbox
                .get_thread("t1")
                .unwrap()
                .unwrap()
                .unread_count,
            0
        );
    }

    #[tokio::test]
    async fn mark_read_on_missing_returns_200_idempotent() {
        // Post 5eb8cc07 mutations are idempotent — a missing thread row
        // returns 200 (noop success) instead of 404 so the UI's optimistic
        // patch doesn't flicker back to unread. Reconciliation happens on
        // the next list refetch.
        let app = build_router(fresh_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/threads/nope/read")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn list_conversations_returns_arrivals() {
        let state = fresh_state();
        for i in 0..3 {
            state
                .mailbox
                .record_message_arrival(&MessageArrival {
                    thread_id: &format!("t{i}"),
                    user: "u@x.com",
                    subject: "Subj",
                    senders_csv: "x@y.z",
                    latest_date: i as i64 * 100,
                    latest_preview: "preview",
                    category: "inbox",
                    unread: true,
                    is_own: false,
                })
                .unwrap();
        }
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/users/u@x.com/conversations:list")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"limit":10}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = body_string(resp).await;
        // reverse chronological → t2 first
        assert!(body.contains(r#""thread_id":"t2""#));
    }

    /// Smoke every business route — verifies no 404 from a router-
    /// resolution bug. Each route is hit with a request that should
    /// land on the handler; expected statuses are documented inline
    /// (the handler's own 204/404 logic is what we then assert).
    #[tokio::test]
    async fn every_route_resolves_no_404() {
        let state = fresh_state();
        // Seed one thread + one message so the routes have a real
        // target to flip / read.
        state
            .mailbox
            .deliver_message(
                &arr("t1", "u@x.com", true),
                "m1",
                b"{}",
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref: "1785000000.M1P1.host",
                    uid: 1,
                    flags: 0,
                    modseq: 1,
                },
            )
            .unwrap();

        struct Probe {
            method: Method,
            uri: &'static str,
            allowed: &'static [u16],
        }
        let probes: &[Probe] = &[
            // Conversations
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/conversations:list",
                allowed: &[200, 415, 422],
            }, // 415/422 if empty body, 200 with body
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/categories",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/conversations/unseen-count",
                allowed: &[200],
            },
            // Thread read
            Probe {
                method: Method::GET,
                uri: "/v1/users/u@x.com/threads/t1/messages",
                allowed: &[200],
            },
            // Thread mutations (return 204 on existing tid, 404 on missing)
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/read",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/pin",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unpin",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/star",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unstar",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/archive",
                allowed: &[200],
            },
            Probe {
                method: Method::POST,
                uri: "/v1/users/u@x.com/threads/t1/unarchive",
                allowed: &[200],
            },
            Probe {
                method: Method::DELETE,
                uri: "/v1/users/u@x.com/threads/t1",
                allowed: &[200],
            }, // delete after archive may already be gone
            // Probes
            Probe {
                method: Method::GET,
                uri: "/v1/healthz",
                allowed: &[200],
            },
            Probe {
                method: Method::GET,
                uri: "/v1/readyz",
                allowed: &[200],
            },
        ];

        for p in probes {
            let app = build_router(state.clone());
            let body = if p.method == Method::POST && p.uri.ends_with(":list") {
                Body::from(r#"{"limit":10}"#)
            } else {
                Body::empty()
            };
            let resp = app
                .oneshot(
                    Request::builder()
                        .method(p.method.clone())
                        .uri(p.uri)
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            let code = resp.status().as_u16();
            assert!(
                p.allowed.contains(&code),
                "{} {} returned {code}, expected {:?}",
                p.method,
                p.uri,
                p.allowed
            );
            assert_ne!(code, 404, "router did not match: {} {}", p.method, p.uri);
        }
    }
}

#[cfg(test)]
mod input_reporting_tests {
    //! A maintenance route must not answer the same thing to "nothing to do"
    //! and "nothing seen".
    //!
    //! `backfill-threading` answered
    //! `{"merged_threads":0,"moved_messages":0,"msgids_indexed":9}` while it
    //! was enumerating a zset nothing writes. Every number was true and the
    //! response was unreadable: the `9` was the only sign the walk was blind,
    //! and it took two failed repair attempts to notice. The counters that
    //! say what was *looked at* are what make a row of zeros legible, and a
    //! comment cannot enforce that they stay.
    use super::tests::{arr, fresh_state};
    use std::sync::Arc;

    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    async fn post_json(state: &Arc<FastcoreState>, uri: &str) -> serde_json::Value {
        let resp = build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn state_with_a_mailbox() -> Arc<FastcoreState> {
        let state = fresh_state();
        // Every maintenance route starts from `list_account_addresses`, so a
        // store with threads and no account is one where they all report
        // zero — which is itself the ambiguity under test, and the reason
        // `accounts` is now in the response.
        state
            .mailbox
            .upsert_account(
                "u@x.com",
                r#"{"address":"u@x.com","active":true,"created_at":0}"#,
            )
            .expect("account");
        for tid in ["t1", "t2", "t3"] {
            state
                .mailbox
                .record_message_arrival(&arr(tid, "u@x.com", true))
                .expect("record");
        }
        state
    }

    /// The route the lesson came from. Its zeros must be readable.
    #[tokio::test]
    async fn backfill_threading_says_what_it_enumerated() {
        let empty = post_json(&fresh_state(), "/v1/admin/backfill-threading").await;
        let full = post_json(&state_with_a_mailbox(), "/v1/admin/backfill-threading").await;

        assert_eq!(
            empty["threads_enumerated"], 0,
            "an empty store enumerates nothing"
        );
        assert_ne!(
            full["threads_enumerated"],
            serde_json::json!(0),
            "a populated store must report the threads it walked — this is \
             the field whose absence made `merged_threads: 0` ambiguous"
        );
        assert_ne!(
            empty, full,
            "the two runs must be distinguishable from the response alone"
        );
    }

    /// Same property, stated once per route so a new one cannot quietly
    /// skip it. Each entry is a route whose result fields are all counts of
    /// *work done*, which are zero in both situations.
    #[tokio::test]
    async fn every_work_route_distinguishes_no_input_from_no_work() {
        let routes = [
            "/v1/admin/backfill-threading",
            "/v1/admin/maintenance:backfill-thread-importance",
            "/v1/admin/maintenance:backfill-triage",
        ];
        for uri in routes {
            let empty = post_json(&fresh_state(), uri).await;
            let full = post_json(&state_with_a_mailbox(), uri).await;
            assert_ne!(
                empty, full,
                "{uri} answers identically whether or not there is anything \
                 to look at, so a zero from it cannot be interpreted"
            );
        }
    }
}
