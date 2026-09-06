//! Read-only Lua bundle validation and migration comparison against a maildir.
use mailrs_fraud::{Facts, Findings, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, MAX_SOURCE, Rules};
use std::{
    collections::HashMap,
    fs,
    io::Read,
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
fn compare(baseline: &str, candidate: &str, root: &str, limit: usize) -> Result<(), String> {
    let mut before = if baseline == "rust" {
        None
    } else {
        Some(Rules::compile(&source(baseline)?)?)
    };
    let mut after = Rules::compile(&source(candidate)?)?;
    let p = policy()?;
    let mut files = vec![];
    paths(Path::new(root), &mut files, limit)?;
    files.sort();
    if files.is_empty() {
        return Err("no maildir messages found".into());
    }
    let mut domain_counts = HashMap::<String, u64>::new();
    let mut replies = HashMap::<String, std::collections::HashSet<String>>::new();
    // History from this read-only corpus, supplied identically to both engines.
    for file in &files {
        let mut header = vec![];
        fs::File::open(file)
            .map_err(|e| e.to_string())?
            .take(64 * 1024)
            .read_to_end(&mut header)
            .map_err(|e| e.to_string())?;
        let from = mailrs_inbound::from_header(&header);
        let host = mailrs_fraud::impersonation::address_of(&from)
            .and_then(|a| a.rsplit('@').next())
            .unwrap_or("");
        let reg = mailrs_fraud::brand::registrable(host);
        *domain_counts.entry(reg.clone()).or_default() += 1;
        if let Some(reply) = reply_key(host, &mailrs_inbound::identity::reply_to_address(&header)) {
            replies.entry(reply).or_default().insert(reg);
        }
    }
    let mut changed = 0;
    let mut added = 0;
    let mut released = 0;
    let mut held = 0;
    let mut samples = vec![];
    for (i, file) in files.iter().enumerate() {
        if i % 25 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if fs::metadata(file).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err(format!("message exceeds 32 MiB: {}", file.display()));
        }
        let raw = fs::read(file).map_err(|e| e.to_string())?;
        let from = mailrs_inbound::from_header(&raw);
        let host = mailrs_fraud::impersonation::address_of(&from)
            .and_then(|a| a.rsplit('@').next())
            .unwrap_or("");
        let reg = mailrs_fraud::brand::registrable(host);
        let subject = mailrs_inbound::subject_header(&raw);
        let to = mailrs_inbound::identity::to_display_name(&raw);
        let x = mailrs_inbound::x_mailer_header(&raw);
        let deception = mailrs_inbound::deception_in_identity(&raw);
        let name_deception = mailrs_inbound::deception_in_display_name(&raw);
        let reply = reply_key(host, &mailrs_inbound::identity::reply_to_address(&raw));
        let parsed = mailrs_mime::parse(&raw);
        let f = Facts {
            from: &from,
            domain: host,
            registrable: &reg,
            subject: &subject,
            to_display: &to,
            x_mailer: x.as_deref(),
            domain_seen: domain_counts
                .get(&reg)
                .copied()
                .unwrap_or(1)
                .saturating_sub(1),
            reply_rotation: reply
                .as_ref()
                .and_then(|r| replies.get(r))
                .map_or(0, |s| s.len() as u32),
            has_zero_width: deception.unjustified_zero_width,
            has_zero_width_in_name: name_deception.unjustified_zero_width,
            has_bidi_override: deception.bidi_override,
            has_zero_width_inside_a_word: deception.zero_width_inside_a_word,
            has_executable_attachment: mailrs_fraud::attachment::any_executable(
                parsed.attachments().filter_map(|p| p.attachment_filename()),
            ),
            ..Facts::default()
        };
        let old = match &mut before {
            Some(r) => {
                let v = r.classify(&f, &p);
                if !v.errors.is_empty() {
                    return Err(format!("baseline: {:?}", v.errors));
                }
                v.findings
            }
            None => mailrs_fraud::scan(&f, &p),
        };
        let new = after.classify(&f, &p);
        if !new.errors.is_empty() {
            return Err(format!("candidate: {:?}", new.errors));
        }
        let old_hold = old.hold_worthy();
        let new_hold = new.findings.hold_worthy();
        held += usize::from(new_hold);
        if rows(&old) != rows(&new.findings) {
            changed += 1;
            added += usize::from(new_hold && !old_hold);
            released += usize::from(old_hold && !new_hold);
            if samples.len() < 50 {
                samples.push(serde_json::json!({"from":from,"subject":subject,"before":old.rules(),"after":new.findings.rules(),"held_before":old_hold,"held_after":new_hold}));
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"checked":files.len(),"changed":changed,"added_holds":added,"removed_holds":released,
        "candidate_holds":held,"baseline":before.as_ref().map_or("rust",|r|r.version()),"candidate":after.version(),
        "account_names_configured":!p.account_names.is_empty(),"samples":samples})
    );
    // A migration must preserve every decision. Intentional rule-only updates
    // compare two Lua bundles and report their diff for review.
    if baseline == "rust" && changed != 0 {
        return Err("Lua migration differs from Rust".into());
    }
    Ok(())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd] if cmd=="bundle" => { print!("{DEFAULT_SOURCE}"); Ok(()) },
        [cmd,path] if cmd=="validate" => {
            let mut rules=Rules::compile(&source(path)?)?;
            let result=rules.classify(&Facts::default(),&policy()?);
            if !result.errors.is_empty(){return Err(format!("{:?}",result.errors));}
            println!("{}",serde_json::json!({"version":rules.version(),"rules":rules.rule_count()}));Ok(())
        },
        [cmd,baseline,candidate,root,limit] if cmd=="compare" => compare(baseline,candidate,root,limit.parse().map_err(|_|"invalid limit")?),
        _=>Err("usage: mailrs-fraud-check bundle | validate FILE | compare rust|builtin|BASELINE FILE|builtin MAILDIR MAX_MESSAGES".into()),
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
