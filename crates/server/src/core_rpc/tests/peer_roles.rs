//! The peer plays the roles it claims, and no others.
//!
//! `Roles` is plain data, so asserting `Roles::peer().smtp == false` only
//! restates a constant. What can actually regress is one level down, in the
//! boot sequence:
//!
//! - a field is added to `Roles` and nothing ever consults it — a knob that
//!   does nothing, which reads as a role being selected when it is not;
//! - a spawn the peer must not perform stops being gated, or arrives ungated,
//!   and the peer quietly starts a role another process owns. Two processes
//!   binding :25 is one bind error and one silent winner; two draining the
//!   outbound queue is a message going out twice.
//!
//! Both are properties of `boot.rs`'s text, so that is what this reads.
//! `CARGO_MANIFEST_DIR`, not a path relative to this file: the predecessor of
//! this test `include_str!`'d `../../../fastcore/src/lib.rs`, which resolved
//! to a directory that has never existed, and it went unnoticed because the
//! whole module sat behind a feature nothing built.

#![cfg(feature = "core-rpc")]

const BOOT_RS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/boot.rs"));

/// The spawns the peer must not perform, each with the process that owns it.
const OWNED_ELSEWHERE: &[(&str, &str)] = &[
    ("spawn_smtp_listeners", "mailrs-receiver"),
    ("spawn_web_server", "mailrs-webapi"),
    ("spawn_outbound_delivery", "mailrs-fastcore-sender"),
    ("spawn_rbl_monitor", "the process that owns SMTP"),
];

/// Field names declared on `Roles`.
fn role_fields(src: &str) -> Vec<String> {
    let start = src
        .find("pub(crate) struct Roles {")
        .expect("boot.rs must declare `struct Roles`");
    let end = src[start..]
        .find("\n}")
        .map(|o| start + o)
        .expect("unterminated Roles struct");
    src[start..end]
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("pub(crate) ")?
                .strip_suffix(": bool,")
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn every_role_actually_gates_something() {
    let fields = role_fields(BOOT_RS);
    assert!(
        fields.len() >= 4,
        "expected the peer's role set to be several fields, found {fields:?}"
    );
    for f in &fields {
        // In a CONDITION, not merely somewhere in the file. The first version
        // of this asserted `contains("roles.<f>")`, and a field read only by a
        // `tracing::debug!` satisfied it — a green light that did not mean what
        // this assertion's own message says. rustc's dead_code covers the field
        // nobody reads at all; what it cannot see is a field read decoratively
        // while gating nothing, so that is the case worth a test.
        let gated = BOOT_RS
            .lines()
            .any(|l| l.trim_start().starts_with(&format!("if roles.{f}")));
        assert!(
            gated,
            "`Roles::{f}` gates nothing — no `if roles.{f}` in boot.rs. A role \
             nobody branches on is a process claiming to select what it in fact \
             always does; gate the subsystem on it, or drop the field."
        );
    }
}

#[test]
fn each_role_the_peer_declines_is_gated() {
    for (spawner, owner) in OWNED_ELSEWHERE {
        let call = format!("{spawner}(");
        let at = BOOT_RS
            .find(&call)
            .unwrap_or_else(|| panic!("boot.rs no longer calls {spawner} — has it moved?"));

        // Walk back to the `if` that encloses this call, by brace depth rather
        // than by taking the nearest preceding `if`: the nearest one is often a
        // sibling block that closed before this line, and this test exists to
        // catch exactly the case where the real enclosing guard is gone.
        let before = &BOOT_RS[..at];
        let mut depth = 0i32;
        let mut guard = None;
        for line in before.lines().rev() {
            depth += line.matches('}').count() as i32;
            depth -= line.matches('{').count() as i32;
            if depth < 0 {
                guard = Some(line.trim().to_owned());
                break;
            }
            depth = depth.max(0);
        }
        let guard = guard.unwrap_or_else(|| {
            panic!("{spawner} is not inside any block — it runs unconditionally")
        });
        assert!(
            guard.contains("roles."),
            "{spawner} is enclosed by `{guard}`, which is not a role gate. \
             {owner} owns this in the peer topology, and an ungated spawn here \
             means the peer starts it too."
        );
    }
}

#[test]
fn the_peer_drains_the_spool() {
    // The one role the peer turns ON that the all-roles build leaves to an env
    // var. It is here because the failure is invisible: a peer that does not
    // drain serves every read correctly and indexes no arriving mail, so the
    // inbox stops growing and nothing reports an error.
    assert!(
        crate::boot::Roles::peer().spool_drain,
        "the peer must drain the spool — it is the process behind the receiver"
    );
    assert!(
        BOOT_RS.contains("roles.spool_drain\n        || std::env::var(\"MAILRS_RECEIVER_SPLIT\")"),
        "the spool drain must run for the peer OR the env flag. If the role \
         stopped being an alternative to the flag, a peer deploy that omitted \
         MAILRS_RECEIVER_SPLIT would silently index nothing."
    );
}
