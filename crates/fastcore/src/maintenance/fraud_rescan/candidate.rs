//! A candidate pack for a dry run: what re-judging with it would change
//! against what is stored now.
//!
//! The pack is named by its sha256 and read from beside the active rules
//! file, where the publishing script puts it, so the route takes no path.
//! The hash is checked against the content through the compiled version,
//! which is the same hash.

use std::io::Read;
use std::path::PathBuf;

use super::{Action, RescanQuery};

/// The candidate's source, with `q` set to judge it the way the re-judge
/// after activation does; `None` when no candidate was asked for. Refuses
/// anything but a dry run.
pub(super) fn prepare(q: &mut RescanQuery) -> Result<Option<String>, String> {
    let Some(hash) = q.candidate.clone() else {
        return Ok(None);
    };
    if !q.dry_run {
        return Err("a candidate pack is judged dry only".into());
    }
    let path = path_for(&hash)?;
    let mut source = String::new();
    std::fs::File::open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take((mailrs_fraud_lua::MAX_SOURCE + 1) as u64)
        .read_to_string(&mut source)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rules = mailrs_fraud_lua::Rules::compile(&source)?;
    if rules.version() != format!("lua:{hash}") {
        return Err(format!("{} is not the pack {hash}", path.display()));
    }
    let probe = rules.classify(&Default::default(), &Default::default());
    if !probe.errors.is_empty() {
        return Err(format!("invalid rules: {:?}", probe.errors));
    }
    q.action = Action::Hold;
    Ok(Some(source))
}

fn path_for(hash: &str) -> Result<PathBuf, String> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("candidate must be a lowercase sha256".into());
    }
    let active = std::env::var_os("MAILRS_FRAUD_RULES_FILE")
        .ok_or("MAILRS_FRAUD_RULES_FILE is unset: there is no rules directory")?;
    let dir = PathBuf::from(active)
        .parent()
        .map(PathBuf::from)
        .ok_or("MAILRS_FRAUD_RULES_FILE has no directory")?;
    Ok(dir.join(format!("candidate-{hash}.lua")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_sha256_names_a_candidate() {
        for bad in ["../active", "ABCDEF", &"A".repeat(64), &"g".repeat(64)] {
            assert!(path_for(bad).is_err(), "{bad}");
        }
    }
}
