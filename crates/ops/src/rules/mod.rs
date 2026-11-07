//! The phase-1 rules: what each one reads, when it holds, and what the
//! person who gets it should do first.
//!
//! A rule is a pure function of one [`Snapshot`]; everything with memory
//! (how long a condition has held, whether it was notified) is the state
//! machine's.

mod core;
mod daemon;
mod data;
mod processes;
mod text;

use crate::readings::Snapshot;

pub use text::Text;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }

    /// How often a firing alert is sent again.
    pub fn repeat_secs(self) -> i64 {
        match self {
            Severity::Critical => 4 * 3600,
            Severity::Warning => 24 * 3600,
        }
    }
}

/// Where a rule's values come from. All rules of one source that cannot be
/// read are reported as one "cannot read <source>".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Fastcore,
    Webapi,
    Receiver,
    Sender,
    Fetch,
    Backup,
    /// The daemon's own records.
    Daemon,
    /// Reports of every process.
    Reports,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Fastcore => "fastcore",
            Source::Webapi => "webapi",
            Source::Receiver => "receiver",
            Source::Sender => "sender",
            Source::Fetch => "fetch",
            Source::Backup => "backup",
            Source::Daemon => "daemon",
            Source::Reports => "reports",
        }
    }

    pub fn of_process(name: &str) -> Option<Source> {
        match name {
            "fastcore" => Some(Source::Fastcore),
            "webapi" => Some(Source::Webapi),
            "receiver" => Some(Source::Receiver),
            "sender" => Some(Source::Sender),
            "fetch" => Some(Source::Fetch),
            "backup" => Some(Source::Backup),
            _ => None,
        }
    }
}

/// What has to be working for a rule's own notice to mean anything. While
/// the dependency's alert is active the rule is still tracked, but its
/// notices are filed under that alert instead of being sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dep {
    KevyServer,
    ReportsArriving,
}

/// One rule's verdict on one object.
#[derive(Debug, Clone, PartialEq)]
pub enum Eval {
    Holds {
        value: String,
        threshold: String,
        /// Above the rule's own level, for a rule with two thresholds.
        severity: Option<Severity>,
    },
    Clear {
        value: String,
    },
    /// Not a value. `source_down` when the whole source could not be read,
    /// so the source's own notice speaks for it.
    Unreadable {
        why: String,
        source_down: bool,
    },
}

impl Eval {
    pub fn holds(value: impl ToString, threshold: impl ToString) -> Self {
        Eval::Holds {
            value: value.to_string(),
            threshold: threshold.to_string(),
            severity: None,
        }
    }

    pub fn clear(value: impl ToString) -> Self {
        Eval::Clear {
            value: value.to_string(),
        }
    }

    pub fn field(why: impl ToString) -> Self {
        Eval::Unreadable {
            why: why.to_string(),
            source_down: false,
        }
    }

    pub fn down(why: impl ToString) -> Self {
        Eval::Unreadable {
            why: why.to_string(),
            source_down: true,
        }
    }
}

pub struct Rule {
    pub name: &'static str,
    pub source: Source,
    pub severity: Severity,
    pub for_secs: i64,
    pub clear_secs: i64,
    /// `None`: notified once per firing; otherwise the severity's default
    /// or this value.
    pub repeat_secs: Option<i64>,
    pub depends: Option<Dep>,
    /// Sent during maintenance: the data is in danger, or the maintenance
    /// state itself is the problem.
    pub during_maintenance: bool,
    /// What kind of thing the objects are ("process", "account" ...): the
    /// only part of the object a minimal push names.
    pub object_kind: &'static str,
    pub text: &'static Text,
    pub eval: fn(&Snapshot) -> Vec<(String, Eval)>,
}

impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl Rule {
    pub fn repeat(&self, severity: Severity) -> Option<i64> {
        self.repeat_secs.map(|r| match r {
            0 => severity.repeat_secs(),
            n => n,
        })
    }
}

/// Repeat at the severity's default interval.
pub(crate) const DEFAULT_REPEAT: Option<i64> = Some(0);

/// The 17 rules of phase 1 and the fraud counter watch, then the rules that speak for a whole source
/// or a dependency.
pub fn all() -> Vec<&'static Rule> {
    let mut v: Vec<&'static Rule> = Vec::new();
    v.extend(core::RULES);
    v.extend(data::RULES);
    v.extend(processes::RULES);
    v.extend(daemon::RULES);
    v
}

/// The rules standing for a source or a dependency rather than a symptom.
pub fn source_rule_name(source: Source) -> String {
    format!("source_unreadable.{}", source.as_str())
}

pub const KEVY_SERVER_UNAVAILABLE: &str = "kevy_server_unavailable";
pub const NO_REPORTS: &str = "no_reports";
pub const CHANNEL_FAILING: &str = "channel_failing";

pub use daemon::{UNKNOWN_WAIT, source_rules, source_text};

#[cfg(test)]
mod tests;
