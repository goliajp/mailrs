//! Read-only Lua bundle validation and migration comparison against a maildir.
// a file directly under src/bin/ would be built as a binary of its own
#[path = "mailrs-fraud-check/judge.rs"]
mod judge;
#[path = "mailrs-fraud-check/threads.rs"]
mod threads;

use judge::{corpus, judge};
use mailrs_fraud::{Facts, Findings, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, MAX_SOURCE, Rules};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

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

fn rows(f: &Findings) -> Vec<(String, String, u64, bool)> {
    let mut rows: Vec<_> = f
        .iter()
        .map(|f| {
            (
                f.rule.clone(),
                f.layer.as_str().into(),
                f.score.to_bits(),
                f.holds,
            )
        })
        .collect();
    rows.sort();
    rows
}
/// What one rule bundle held: per hold-level rule, the held letters it
/// matched (a letter matching three counts once for each), and per month of
/// the letter's own Date header, the held letters. Counts only: nothing
/// that names a sender or a subject.
#[derive(Default)]
struct Held {
    by_rule: BTreeMap<String, u64>,
    by_month: BTreeMap<String, u64>,
}

impl Held {
    fn add(&mut self, f: &Findings, month: &str) {
        if !f.hold_worthy() {
            return;
        }
        for r in f.iter().filter(|r| r.holds) {
            *self.by_rule.entry(r.rule.clone()).or_default() += 1;
        }
        *self.by_month.entry(month.to_string()).or_default() += 1;
    }
}

/// What changed between the two bundles, message by message.
#[derive(Default)]
struct Tally {
    changed: usize,
    added: usize,
    released: usize,
    held: usize,
    samples: Vec<serde_json::Value>,
    /// Every changed message's added (`+rule`) and dropped (`-rule`)
    /// rules: the full count the fifty samples cannot give.
    by_rule: BTreeMap<String, u64>,
    /// One JSON line per checked message, when asked for.
    list: Option<BufWriter<fs::File>>,
}

impl Tally {
    fn record(
        &mut self,
        path: &str,
        (from, subject): (&str, &str),
        (old, old_shadow): (&Findings, &Findings),
        (new, new_shadow): (&Findings, &Findings),
        thread_held_now: Option<bool>,
    ) -> Result<(), String> {
        let (old_hold, new_hold) = (old.hold_worthy(), new.hold_worthy());
        self.held += usize::from(new_hold);
        if let Some(list) = &mut self.list {
            let mut line = serde_json::json!({"path":path,"rules_before":old.rules(),"rules_after":new.rules(),
                "held_before":old_hold,"held_after":new_hold,
                "shadow_before":old_shadow.rules(),"shadow_after":new_shadow.rules()});
            if let Some(now) = thread_held_now {
                line["thread_held_now"] = serde_json::json!(now);
            }
            writeln!(list, "{line}").map_err(|e| e.to_string())?;
        }
        if rows(old) == rows(new) {
            return Ok(());
        }
        self.changed += 1;
        self.added += usize::from(new_hold && !old_hold);
        self.released += usize::from(old_hold && !new_hold);
        let before: BTreeSet<&str> = old.rules().into_iter().collect();
        let after: BTreeSet<&str> = new.rules().into_iter().collect();
        for r in after.difference(&before) {
            *self.by_rule.entry(format!("+{r}")).or_default() += 1;
        }
        for r in before.difference(&after) {
            *self.by_rule.entry(format!("-{r}")).or_default() += 1;
        }
        if self.samples.len() < 50 {
            self.samples.push(serde_json::json!({"from":from,"subject":subject,"before":old.rules(),"after":new.rules(),"held_before":old_hold,"held_after":new_hold}));
        }
        Ok(())
    }
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

fn paths(root: &Path, out: &mut Vec<PathBuf>, max: usize) -> Result<(), String> {
    if out.len() > max {
        return Err(format!(
            "maildir exceeds {max} messages; use a larger explicit bound"
        ));
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_dir() && entry.file_name() != "tmp" {
            paths(&entry.path(), out, max)?;
        } else if ty.is_file() && root.file_name().is_some_and(|s| s == "cur" || s == "new") {
            out.push(entry.path());
        }
        if out.len() > max {
            return Err(format!(
                "maildir exceeds {max} messages; use a larger explicit bound"
            ));
        }
    }
    Ok(())
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
    let mut files = vec![];
    paths(Path::new(root), &mut files, limit)?;
    files.sort();
    if files.is_empty() {
        return Err("no maildir messages found".into());
    }
    let mut c = corpus(root, &files)?;
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
    for (i, file) in files.iter().enumerate() {
        if i % 25 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let j = judge(&mut c, &mut before, &mut after, file)?;
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
        let rel = file.strip_prefix(root).unwrap_or(file).to_string_lossy();
        let mut thread_held_now = None;
        if let Some(t) = &held_now {
            let (now, key) = t.held_now(&reader_of(Path::new(root), file), &threads::head(file)?);
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
        )?;
    }
    if let Some(list) = &mut tally.list {
        list.flush().map_err(|e| e.to_string())?;
    }
    let mut report = serde_json::json!({"checked":files.len(),"changed":tally.changed,"added_holds":tally.added,"removed_holds":tally.released,
        "candidate_holds":tally.held,"baseline":before.as_ref().map_or("rust",|r|r.version()),"candidate":after.version(),
        "account_names_configured":!c.policy.account_names.is_empty(),
        "holds_by_rule":candidate_held.by_rule,"holds_by_month":candidate_held.by_month,
        "baseline_holds_by_rule":baseline_held.by_rule,"baseline_holds_by_month":baseline_held.by_month,
        "changed_by_rule":tally.by_rule,
        "shadow_rules":after.shadow_rules(),"shadow_hits_by_rule":candidate_shadow,
        "baseline_shadow_rules":before.as_ref().map_or(&[][..],|r|r.shadow_rules()),"baseline_shadow_hits_by_rule":baseline_shadow,
        "samples":tally.samples});
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
const USAGE: &str = "usage: mailrs-fraud-check bundle | validate FILE | compare rust|builtin|BASELINE FILE|builtin MAILDIR MAX_MESSAGES [--list OUT.jsonl] [--held-threads HELD.tsv [--released-threads RELEASED.tsv]]";

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
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
