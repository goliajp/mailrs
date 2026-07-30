//! No production path may read a legacy thread zset.
//!
//! `keys::all_user_thread_zsets` is the list `drop-legacy-zsets` deletes.
//! Reading one is neither a compile error nor a runtime error: a `zrange`
//! over a deleted key returns empty, so the caller sees "nothing to do" and
//! reports success. That is how `backfill-threading` enumerated 9 messages
//! against 30,562 declared rows and answered 200 for however long, and how
//! the Bayes bootstrap, the importance sweep, the uid-index backfill and the
//! contacts backfill all became no-ops nobody noticed.
//!
//! Measured on prod 2026-07-31: thirteen of the fifteen zsets were empty on
//! every account — `.claude/notes/legacy-zset-census-2026-07-31.md`.
//!
//! A grep, because the property is "this name does not appear next to a read
//! operation" and no type can carry it. Exemptions are the tooling that
//! exists to *observe* the legacy keys, each named individually so adding one
//! is a deliberate act with a reason attached.

use std::path::{Path, PathBuf};

/// Functions allowed to read a legacy zset, and why.
const EXEMPT_FNS: &[(&str, &str)] = &[
    ("drop_legacy_zsets_route", "deletes them; counts first"),
    ("legacy_zset_census_route", "reports their cardinality"),
    (
        "sent_axis_shadow_route",
        "compares zset against declared axis",
    ),
    ("shadow_read_route", "compares zset against declared axis"),
];

const LEGACY_KEY_FNS: &[&str] = &[
    "user_threads_by_activity",
    "user_threads_by_category",
    "user_threads_inbox",
    "user_threads_junk",
    "user_threads_starred",
    "user_threads_archived",
    "user_threads_pinned",
    "user_threads_has_unread",
    "user_threads_has_action",
    "user_threads_sent",
    "all_user_thread_zsets",
];

const READ_OPS: &[&str] = &["zrevrange", "zrange(", "zcard", "zscore", "zrangebyscore"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The enclosing `fn` name for a line, by scanning backwards.
fn enclosing_fn(lines: &[&str], idx: usize) -> String {
    for line in lines[..=idx].iter().rev() {
        let t = line.trim_start();
        let rest = t
            .strip_prefix("pub(crate) async fn ")
            .or_else(|| t.strip_prefix("pub async fn "))
            .or_else(|| t.strip_prefix("pub(crate) fn "))
            .or_else(|| t.strip_prefix("pub fn "))
            .or_else(|| t.strip_prefix("async fn "))
            .or_else(|| t.strip_prefix("fn "));
        if let Some(rest) = rest {
            return rest
                .split(['(', '<', ' '])
                .next()
                .unwrap_or_default()
                .to_string();
        }
    }
    String::new()
}

#[test]
fn no_production_path_reads_a_legacy_thread_zset() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "found no sources under {}",
        src.display()
    );

    let mut offences = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if !LEGACY_KEY_FNS.iter().any(|k| line.contains(k)) {
                continue;
            }
            // The key name and a read op within a few lines — these calls are
            // usually spread over two or three.
            let window = lines[i..(i + 8).min(lines.len())].join(" ");
            if !READ_OPS.iter().any(|op| window.contains(op)) {
                continue;
            }
            let f = enclosing_fn(&lines, i);
            if EXEMPT_FNS.iter().any(|(name, _)| *name == f) {
                continue;
            }
            offences.push(format!(
                "{}:{} in `{}`",
                path.file_name().unwrap_or_default().to_string_lossy(),
                i + 1,
                f
            ));
        }
    }

    assert!(
        offences.is_empty(),
        "these read a zset that `drop-legacy-zsets` deletes, so they will \
         silently see nothing and report success:\n  {}\n\nUse the declared \
         table instead: `all_thread_ids_for_user`, \
         `list_thread_ids_by_category_via_table`, \
         `list_thread_ids_by_bucket_unsent_via_table`, or \
         `list_thread_ids_by_flag_via_table`. If the read is deliberate \
         tooling that observes the legacy keys, add its function to \
         EXEMPT_FNS with a reason.",
        offences.join("\n  ")
    );
}

/// The exemption list is a list of reasons, not a list of names.
#[test]
fn every_exemption_states_why() {
    for (name, why) in EXEMPT_FNS {
        assert!(!why.trim().is_empty(), "exemption `{name}` has no reason");
    }
}
