//! Per-thread VMs, atomic-file reloads and last-known-good recovery.
use crate::{DEFAULT_SOURCE, MAX_SOURCE, Rules};
use mailrs_fraud::{Facts, Findings, Policy};
use std::{
    cell::RefCell,
    fs::File,
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};

/// Findings and the exact version that evaluated them.
pub struct Scan {
    /// Findings for the existing Review/Junk action machinery.
    pub findings: Findings,
    /// Content hash, including fallback identity if execution failed.
    pub version: String,
}

struct Active {
    current: Rules,
    previous: Option<Rules>,
    source: String,
    checked: Instant,
    path: Option<PathBuf>,
}

impl Active {
    fn new() -> Self {
        Self {
            current: Rules::compile(DEFAULT_SOURCE).expect("embedded fraud rules must compile"),
            previous: None,
            source: DEFAULT_SOURCE.into(),
            checked: Instant::now() - Duration::from_secs(10),
            path: std::env::var_os("MAILRS_FRAUD_RULES_FILE").map(PathBuf::from),
        }
    }
    fn refresh(&mut self) {
        if self.checked.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.checked = Instant::now();
        let Some(path) = &self.path else {
            return;
        };
        let source = (|| -> Result<String, String> {
            let file = File::open(path).map_err(|e| e.to_string())?;
            let mut source = String::new();
            file.take((MAX_SOURCE + 1) as u64)
                .read_to_string(&mut source)
                .map_err(|e| e.to_string())?;
            Ok(source)
        })();
        match source {
            Ok(source) if source == self.source => {}
            Ok(source) => match Rules::compile(&source) {
                Ok(mut candidate) => {
                    let probe = candidate.classify(&Facts::default(), &Policy::default());
                    if !probe.errors.is_empty() {
                        tracing::error!(errors=?probe.errors, "fraud Lua candidate failed; keeping previous rules");
                        return;
                    }
                    tracing::info!(version = candidate.version(), "fraud Lua rules activated");
                    self.previous = Some(std::mem::replace(&mut self.current, candidate));
                    self.source = source;
                }
                Err(error) => {
                    tracing::error!(%error, "fraud Lua compile failed; keeping previous rules")
                }
            },
            Err(error) => {
                tracing::warn!(%error, path=%path.display(), "fraud Lua file unavailable; keeping current rules")
            }
        }
    }
    fn scan(&mut self, facts: &Facts<'_>, policy: &Policy) -> Result<Scan, String> {
        self.refresh();
        let result = self.current.classify(facts, policy);
        if result.errors.is_empty() {
            return Ok(Scan {
                findings: result.findings,
                version: self.current.version().into(),
            });
        }
        tracing::error!(errors=?result.errors, version=self.current.version(), "fraud Lua execution failed; falling back");
        let rejected = self.current.version().to_string();
        let fallback = self
            .previous
            .take()
            .unwrap_or_else(|| Rules::compile(DEFAULT_SOURCE).expect("embedded rules"));
        self.current = fallback;
        let mut recovered = self.current.classify(facts, policy);
        // Successful rules from the failed bundle must not disappear either.
        let mut merged = std::collections::BTreeMap::new();
        for f in recovered.findings.iter().chain(result.findings.iter()) {
            let entry = merged.entry(f.rule.clone()).or_insert_with(|| f.clone());
            if f.holds && !entry.holds || f.score > entry.score && f.holds == entry.holds {
                *entry = f.clone();
            }
        }
        recovered.findings = Findings::new();
        for f in merged.into_values() {
            recovered.findings.push(f);
        }
        if !recovered.errors.is_empty() {
            return Err(format!(
                "fraud Lua fallback also failed: {:?}",
                recovered.errors
            ));
        }
        Ok(Scan {
            findings: recovered.findings,
            version: format!("{};fallback-from={rejected}", self.current.version()),
        })
    }
}
thread_local! { static ACTIVE: RefCell<Active> = RefCell::new(Active::new()); }

/// Score using the current Lua bundle. Check its configured file at most once
/// every five seconds per worker. Missing/invalid files keep the working VM.
pub fn scan(facts: &Facts<'_>, policy: &Policy) -> Result<Scan, String> {
    ACTIVE.with(|active| active.borrow_mut().scan(facts, policy))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_failure_recovers_without_double_scoring() {
        let mut active = Active::new();
        let source = format!(
            "{}\nrule('broken','content',1,true,function(m) if m.from ~= '' then error('broken') end end)",
            DEFAULT_SOURCE
        );
        active.current = Rules::compile(&source).unwrap();
        let f = Facts {
            from: "ChatGPT <admin@heavenerandassociates.com>",
            ..Facts::default()
        };
        let recovered = active.scan(&f, &Policy::default()).unwrap();
        let baseline = Rules::compile(DEFAULT_SOURCE)
            .unwrap()
            .classify(&f, &Policy::default());
        assert!(recovered.version.contains("fallback-from"));
        assert_eq!(recovered.findings.len(), baseline.findings.len());
        assert_eq!(recovered.findings.score(), baseline.findings.score());
        assert!(recovered.findings.hold_worthy());
    }

    #[test]
    fn invalid_reload_keeps_working_rules() {
        let path =
            std::env::temp_dir().join(format!("mailrs-fraud-reload-{}.lua", std::process::id()));
        std::fs::write(&path, "invalid lua !!!").unwrap();
        let mut active = Active::new();
        active.path = Some(path.clone());
        active.refresh();
        let f = Facts {
            from: "ChatGPT <admin@heavenerandassociates.com>",
            ..Facts::default()
        };
        assert!(
            active
                .scan(&f, &Policy::default())
                .unwrap()
                .findings
                .hold_worthy()
        );
        std::fs::write(
            &path,
            "rule('test', 'identity', 1, false, function(m) return 'changed' end)",
        )
        .unwrap();
        active.checked -= Duration::from_secs(10);
        let changed = active.scan(&f, &Policy::default()).unwrap();
        assert!(changed.findings.has("test"));
        assert!(!changed.findings.hold_worthy());
        std::fs::remove_file(path).unwrap();
    }
}
