//! Per-thread VMs, file reloads, and last-known-good recovery.

use std::cell::RefCell;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mailrs_reputation::Facts;

use crate::{DEFAULT_SOURCE, Input, MAX_SOURCE, Rules, Verdict};

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
            current: Rules::compile(DEFAULT_SOURCE).expect("embedded screening rules must compile"),
            previous: None,
            source: DEFAULT_SOURCE.into(),
            checked: Instant::now() - Duration::from_secs(10),
            path: std::env::var_os("MAILRS_SCREENING_RULES_FILE").map(PathBuf::from),
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
                    let empty = Facts::default();
                    let probe = candidate.evaluate(&Input {
                        outside: true,
                        parts: &[],
                        reputation: &empty,
                    });
                    if !probe.errors.is_empty() {
                        tracing::error!(errors = ?probe.errors, "screening Lua candidate failed; keeping previous rules");
                        return;
                    }
                    tracing::info!(
                        version = candidate.version(),
                        "screening Lua rules activated"
                    );
                    self.previous = Some(std::mem::replace(&mut self.current, candidate));
                    self.source = source;
                }
                Err(error) => {
                    tracing::error!(%error, "screening Lua compile failed; keeping previous rules")
                }
            },
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "screening Lua file unavailable; keeping current rules")
            }
        }
    }

    fn evaluate(&mut self, input: &Input<'_>) -> Result<Verdict, String> {
        self.refresh();
        let result = self.current.evaluate(input);
        if result.errors.is_empty() {
            return Ok(Verdict {
                hits: result.hits,
                version: self.current.version().into(),
            });
        }
        tracing::error!(errors = ?result.errors, version = self.current.version(), "screening Lua execution failed; falling back");
        let rejected = self.current.version().to_string();
        let fallback = self
            .previous
            .take()
            .unwrap_or_else(|| Rules::compile(DEFAULT_SOURCE).expect("embedded rules"));
        self.current = fallback;
        let recovered = self.current.evaluate(input);
        if !recovered.errors.is_empty() {
            return Err(format!(
                "screening Lua fallback also failed: {:?}",
                recovered.errors
            ));
        }
        Ok(Verdict {
            hits: recovered.hits,
            version: format!("{};fallback-from={rejected}", self.current.version()),
        })
    }
}

thread_local! { static ACTIVE: RefCell<Active> = RefCell::new(Active::new()); }

/// Judge with the current bundle. The configured file
/// (`MAILRS_SCREENING_RULES_FILE`) is checked at most once every five
/// seconds per thread; a missing or invalid file keeps the working bundle.
pub fn evaluate(input: &Input<'_>) -> Result<Verdict, String> {
    ACTIVE.with(|active| active.borrow_mut().evaluate(input))
}

/// The version this thread's bundle has after a reload check, for health.
pub fn active_version() -> String {
    ACTIVE.with(|active| {
        let mut a = active.borrow_mut();
        a.refresh();
        a.current.version().to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_eval(a: &mut Active) -> Verdict {
        let f = Facts::default();
        a.evaluate(&Input {
            outside: true,
            parts: &[],
            reputation: &f,
        })
        .unwrap()
    }

    #[test]
    fn a_runtime_failure_falls_back_to_the_previous_bundle() {
        let mut active = Active::new();
        let source = format!(
            "{DEFAULT_SOURCE}\nrule({{id='broken', kind='reputation', action='score', score=1, reason='x', inspired_by='', check=function(m) error('broken') end}})"
        );
        active.current = Rules::compile(&source).unwrap();
        let v = empty_eval(&mut active);
        assert!(v.version.contains("fallback-from"));
        assert!(v.hits.is_empty());
        assert_eq!(active.current.version(), crate::version_of(DEFAULT_SOURCE));
    }

    #[test]
    fn an_invalid_file_keeps_the_working_rules() {
        let path = std::env::temp_dir().join(format!(
            "mailrs-screening-reload-{}.lua",
            std::process::id()
        ));
        std::fs::write(&path, "invalid lua !!!").unwrap();
        let mut active = Active::new();
        active.path = Some(path.clone());
        active.refresh();
        assert_eq!(active.current.version(), crate::version_of(DEFAULT_SOURCE));
        let changed = format!(
            "{}\nrule({{id='always', kind='reputation', action='score', score=1, reason='x', inspired_by='', check=function(m) return 'yes' end}})",
            include_str!("../rules/policy.lua")
        );
        std::fs::write(&path, &changed).unwrap();
        active.checked -= Duration::from_secs(10);
        let v = empty_eval(&mut active);
        assert_eq!(v.version, crate::version_of(&changed));
        assert_eq!(v.hits.len(), 1);
        std::fs::remove_file(path).unwrap();
    }
}
