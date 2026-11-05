//! Read-only Lua bundle validation and migration comparison against a maildir.
// a file directly under src/bin/ would be built as a binary of its own
#[path = "mailrs-fraud-check/judge.rs"]
mod judge;
#[path = "mailrs-fraud-check/placement.rs"]
mod placement;
#[path = "mailrs-fraud-check/tally.rs"]
mod tally;
#[path = "mailrs-fraud-check/threads.rs"]
mod threads;
#[path = "mailrs-fraud-check/walk.rs"]
mod walk;

use judge::{corpus, judge};
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, MAX_SOURCE, Rules};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufWriter, Read, Write},
    path::Path,
};
use tally::{Held, Tally};

fn source(path: &str) -> Result<String, String> {
    if path == "builtin" {
        return Ok(DEFAULT_SOURCE.into());
    }
    let mut source = String::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_SOURCE + 1) as u64)
        .read_to_string(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() > MAX_SOURCE {
        return Err("source too large".into());
    }
    Ok(source)
}
fn reply_key(host: &str, address: &str) -> Option<String> {
    mailrs_fraud::reply_rotation::is_off_domain(host, address)
        .then(|| mailrs_fraud::reply_rotation::rotation_key(address))
}
fn list(var: &str) -> Vec<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
fn policy() -> Result<Policy, String> {
    let mut account_names = list("MAILRS_FRAUD_ACCOUNT_NAMES");
    if let Ok(url) = std::env::var("MAILRS_KEVY_URL") {
        let mut conn = kevy_client::Connection::connect(&url)
            .map_err(|e| format!("cannot read account names: {e}"))?;
        account_names = mailrs_core_sidestate::families::account_names::read(&mut conn);
        if account_names.is_empty() {
            return Err(
                "account-name corpus is unavailable; refusing incomplete comparison".into(),
            );
        }
    }
    Ok(Policy {
        org_names: list("MAILRS_ORG_NAMES"),
        our_domains: list("MAILRS_LOCAL_DOMAINS"),
        allowed_domains: list("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
        account_names,
    })
}

/// `YYYY-MM` of the Date header, in UTC; `unknown` when there is none
/// that parses.
fn month_of(raw: &[u8]) -> String {
    let Some(t) = mailrs_inbound::dates::date_header_epoch(raw) else {
        return "unknown".into();
    };
    // days since 1970-01-01 to a civil year and month (Hinnant's algorithm)
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = match mp < 10 {
        true => mp + 3,
        false => mp - 9,
    };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

/// Whose mail a file is: `<root>/<domain>/<local>/...`. Empty when the
/// path does not say, and then the reader's history reads as empty.
fn reader_of(root: &Path, file: &Path) -> String {
    let mut parts = file
        .strip_prefix(root)
        .ok()
        .into_iter()
        .flat_map(|p| p.components())
        .filter_map(|c| c.as_os_str().to_str());
    match (parts.next(), parts.next()) {
        (Some(domain), Some(local)) => format!("{local}@{domain}"),
        _ => String::new(),
    }
}

/// Messages each shadow rule hit; a rule that hit nothing is listed at
/// zero, so "never fired" reads differently from "not a shadow rule".
fn shadow_counts(rules: Option<&Rules>) -> BTreeMap<String, u64> {
    rules
        .map(|r| r.shadow_rules().iter().map(|id| (id.clone(), 0)).collect())
        .unwrap_or_default()
}

fn compare(
    baseline: &str,
    candidate: &str,
    root: &str,
    limit: usize,
    list: Option<&str>,
    held_now: Option<threads::Threads>,
) -> Result<(), String> {
    let mut before = if baseline == "rust" {
        None
    } else {
        Some(Rules::compile(&source(baseline)?)?)
    };
    let mut after = Rules::compile(&source(candidate)?)?;
    let (mut maildir, files) = walk::Maildir::list(Path::new(root), limit)?;
    if files.is_empty() {
        return Err("no maildir messages found".into());
    }
    let mut c = corpus(root, &files, &mut maildir)?;
    let mut checked = 0usize;
    let mut tally = Tally {
        list: list
            .map(|p| fs::File::create(p).map(BufWriter::new))
            .transpose()
            .map_err(|e| e.to_string())?,
        ..Tally::default()
    };
    let mut baseline_held = Held::default();
    let mut candidate_held = Held::default();
    let mut baseline_shadow = shadow_counts(before.as_ref());
    let mut candidate_shadow = shadow_counts(Some(&after));
    let mut unheld = threads::Unheld::default();
    let mut moves = placement::Moves::default();
    for (i, file) in files.iter().enumerate() {
        if i % 25 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // a file the server has since deleted is counted in the report
        let Some((file, raw)) = maildir.read(file, judge::MAX_MESSAGE + 1)? else {
            continue;
        };
        let file = file.as_path();
        let head = raw[..raw.len().min(16 * 1024)].to_vec();
        let j = judge(&mut c, &mut before, &mut after, file, raw)?;
        checked += 1;
        baseline_held.add(&j.old, &j.month);
        candidate_held.add(&j.new, &j.month);
        for (counts, hits) in [
            (&mut baseline_shadow, &j.old_shadow),
            (&mut candidate_shadow, &j.new_shadow),
        ] {
            for r in hits.rules() {
                *counts.entry(r.to_string()).or_default() += 1;
            }
        }
        let places = (
            placement::place(&j.old, j.whitelisted),
            placement::place(&j.new, j.whitelisted),
        );
        moves.record(places, &j.domain, &j.old, &j.new);
        let rel = file.strip_prefix(root).unwrap_or(file).to_string_lossy();
        let mut thread_held_now = None;
        if let Some(t) = &held_now {
            let (now, key) = t.held_now(&reader_of(Path::new(root), file), &head);
            if j.new.hold_worthy() {
                t.count(&mut unheld, now, key);
            }
            thread_held_now = Some(now);
        }
        tally.record(
            &rel,
            (&j.from, &j.subject),
            (&j.old, &j.old_shadow),
            (&j.new, &j.new_shadow),
            thread_held_now,
            (places.0.as_str(), places.1.as_str()),
        )?;
    }
    if let Some(list) = &mut tally.list {
        list.flush().map_err(|e| e.to_string())?;
    }
    let mut report = serde_json::json!({"listed":files.len(),"checked":checked,
        "rewalks":maildir.rewalks,"vanished":maildir.vanished.len(),"vanished_files":maildir.vanished,
        "changed":tally.changed,"added_holds":tally.added,"removed_holds":tally.released,
        "candidate_holds":tally.held,"baseline":before.as_ref().map_or("rust",|r|r.version()),"candidate":after.version(),
        "account_names_configured":!c.policy.account_names.is_empty(),
        "holds_by_rule":candidate_held.by_rule,"holds_by_month":candidate_held.by_month,
        "baseline_holds_by_rule":baseline_held.by_rule,"baseline_holds_by_month":baseline_held.by_month,
        "changed_by_rule":tally.by_rule,
        "shadow_rules":after.shadow_rules(),"shadow_hits_by_rule":candidate_shadow,
        "baseline_shadow_rules":before.as_ref().map_or(&[][..],|r|r.shadow_rules()),"baseline_shadow_hits_by_rule":baseline_shadow,
        "samples":tally.samples});
    moves.report(&mut report, c.whitelist_checked());
    // what a re-judge after activating would newly hold, given what is held now
    if held_now.is_some() {
        report["would_hold_in_unheld_threads"] = unheld.json();
    }
    println!("{report}");
    // A migration must preserve every decision. Intentional rule-only updates
    // compare two Lua bundles and report their diff for review.
    if baseline == "rust" && tally.changed != 0 {
        return Err("Lua migration differs from Rust".into());
    }
    Ok(())
}
const USAGE: &str = "usage: mailrs-fraud-check bundle | validate FILE | compare rust|builtin|BASELINE FILE|builtin MAILDIR MAX_MESSAGES [--list OUT.jsonl] [--held-threads HELD.tsv [--released-threads RELEASED.tsv]] | summary REPORT.json";

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd] if cmd == "bundle" => {
            print!("{DEFAULT_SOURCE}");
            Ok(())
        }
        [cmd, path] if cmd == "validate" => {
            let mut rules = Rules::compile(&source(path)?)?;
            let result = rules.classify(&Facts::default(), &policy()?);
            if !result.errors.is_empty() {
                return Err(format!("{:?}", result.errors));
            }
            println!(
                "{}",
                serde_json::json!({"version":rules.version(),"rules":rules.rule_count(),"shadow":rules.shadow_rules()})
            );
            Ok(())
        }
        [cmd, path] if cmd == "summary" => {
            let report: serde_json::Value =
                serde_json::from_slice(&fs::read(path).map_err(|e| format!("{path}: {e}"))?)
                    .map_err(|e| format!("{path}: {e}"))?;
            for line in placement::summary(&report) {
                println!("{line}");
            }
            Ok(())
        }
        [cmd, baseline, candidate, root, limit, flags @ ..] if cmd == "compare" => {
            let flag = |name: &str| {
                flags
                    .chunks(2)
                    .find(|p| p.len() == 2 && p[0] == name)
                    .map(|p| p[1].as_str())
            };
            if flags.len() % 2 != 0
                || flags.chunks(2).any(|p| {
                    !["--list", "--held-threads", "--released-threads"].contains(&p[0].as_str())
                })
            {
                return Err(USAGE.into());
            }
            let held_now = flag("--held-threads")
                .map(|h| threads::Threads::load(h, flag("--released-threads")))
                .transpose()?;
            compare(
                baseline,
                candidate,
                root,
                limit.parse().map_err(|_| "invalid limit")?,
                flag("--list"),
                held_now,
            )
        }
        _ => Err(USAGE.into()),
    }
}
fn main() {
    // A read-only tool: without the worker next to it (a development
    // build) a parser that dies takes only this run, so it reads in
    // process and says so.
    match mailrs_parse_worker::Setup::from_env(1) {
        Ok(setup) => mailrs_parse_worker::install("fraud-check", setup),
        Err(e) => eprintln!("parse worker: {e}; parsing in process"),
    }
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
