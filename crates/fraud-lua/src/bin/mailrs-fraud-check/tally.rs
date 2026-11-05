//! What `compare` counts: the holds of each bundle, and what changed between
//! them, message by message.

use mailrs_fraud::Findings;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufWriter, Write},
};

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
pub struct Held {
    pub by_rule: BTreeMap<String, u64>,
    pub by_month: BTreeMap<String, u64>,
}

impl Held {
    pub fn add(&mut self, f: &Findings, month: &str) {
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
pub struct Tally {
    pub changed: usize,
    pub added: usize,
    pub released: usize,
    pub held: usize,
    pub samples: Vec<serde_json::Value>,
    /// Every changed message's added (`+rule`) and dropped (`-rule`)
    /// rules: the full count the fifty samples cannot give.
    pub by_rule: BTreeMap<String, u64>,
    /// One JSON line per checked message, when asked for.
    pub list: Option<BufWriter<fs::File>>,
}

impl Tally {
    pub fn record(
        &mut self,
        path: &str,
        (from, subject): (&str, &str),
        (old, old_shadow): (&Findings, &Findings),
        (new, new_shadow): (&Findings, &Findings),
        thread_held_now: Option<bool>,
        (placed_before, placed_after): (&str, &str),
    ) -> Result<(), String> {
        let (old_hold, new_hold) = (old.hold_worthy(), new.hold_worthy());
        self.held += usize::from(new_hold);
        if let Some(list) = &mut self.list {
            let mut line = serde_json::json!({"path":path,"rules_before":old.rules(),"rules_after":new.rules(),
                "held_before":old_hold,"held_after":new_hold,
                "shadow_before":old_shadow.rules(),"shadow_after":new_shadow.rules(),
                "placement_before":placed_before,"placement_after":placed_after});
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
