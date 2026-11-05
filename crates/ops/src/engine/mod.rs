//! Every rule's state for every object, advanced once per cycle, with each
//! phase change recorded as a fact.

pub mod machine;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use machine::{AlertState, NoticeKind, Phase, Timing};
use serde::Serialize;

use crate::readings::Snapshot;
use crate::rules::{self, Dep, Eval, Rule, Severity};
use crate::store::{Store, Transition};

/// One thing to tell people about.
#[derive(Debug, Clone)]
pub struct Notice {
    pub rule: &'static Rule,
    pub object: String,
    pub kind: NoticeKind,
    pub severity: Severity,
    pub value: String,
    pub threshold: String,
    pub since: i64,
    pub at: i64,
    /// The active alert this one is filed under instead of being sent.
    pub under: Option<&'static str>,
}

/// One alert as `/v1/state` shows it.
#[derive(Debug, Clone, Serialize)]
pub struct AlertView {
    pub rule: &'static str,
    pub object: String,
    pub phase: &'static str,
    pub severity: &'static str,
    pub since: i64,
    pub value: Option<String>,
    pub threshold: Option<String>,
    pub why: Option<String>,
    pub title_en: &'static str,
    pub title_zh: &'static str,
}

pub struct Engine {
    rules: Vec<&'static Rule>,
    states: BTreeMap<(String, String), AlertState>,
    store: Arc<Store>,
    unknown_wait: i64,
}

impl Engine {
    /// The saved states, or each one rebuilt from its last transition.
    pub fn load(store: Arc<Store>, unknown_wait: i64, now: i64) -> crate::store::Result<Self> {
        let mut states = BTreeMap::new();
        for (rule, object, phase, at) in store.last_phases()? {
            if let Some(p) = Phase::parse(&phase) {
                states.insert((rule, object), AlertState::rebuilt(p, at.min(now)));
            }
        }
        for (rule, object, json) in store.load_states()? {
            if let Ok(st) = serde_json::from_str::<AlertState>(&json) {
                states.insert((rule, object), st);
            }
        }
        Ok(Engine {
            rules: rules::all(),
            states,
            store,
            unknown_wait,
        })
    }

    fn timing(&self, rule: &Rule) -> Timing {
        let wait = |secs: i64| match secs {
            rules::UNKNOWN_WAIT => self.unknown_wait,
            s => s,
        };
        Timing {
            for_secs: wait(rule.for_secs),
            clear_secs: rule.clear_secs,
            unknown_wait: self.unknown_wait,
            base: rule.severity,
        }
    }

    /// Advance every alert by `snap` and return what to tell people.
    pub fn evaluate(&mut self, snap: &Snapshot) -> Vec<Notice> {
        let now = snap.now;
        let mut notices = Vec::new();
        for rule in self.rules.clone() {
            let mut evals = (rule.eval)(snap);
            // an object no longer listed is gone, which is not a symptom
            let listed: BTreeSet<String> = evals.iter().map(|(o, _)| o.clone()).collect();
            for ((r, o), st) in &self.states {
                if r == rule.name && !listed.contains(o) && st.phase != Phase::Ok {
                    evals.push((o.clone(), Eval::clear("no longer present")));
                }
            }
            let timing = self.timing(rule);
            for (object, eval) in evals {
                notices.extend(self.advance(rule, &timing, object, &eval, now));
            }
        }
        for n in &mut notices {
            n.under = n.rule.depends.and_then(|d| self.covering(d));
        }
        notices
    }

    fn advance(
        &mut self,
        rule: &'static Rule,
        timing: &Timing,
        object: String,
        eval: &Eval,
        now: i64,
    ) -> Vec<Notice> {
        let key = (rule.name.to_string(), object.clone());
        let st = self
            .states
            .entry(key)
            .or_insert_with(|| AlertState::new(now));
        let step = machine::step(st, eval, now, timing, |sev| rule.repeat(sev));
        let st = st.clone();
        for (from, to, reason) in &step.transitions {
            let t = Transition {
                rule: rule.name.to_string(),
                object: object.clone(),
                from: from.as_str().into(),
                to: to.as_str().into(),
                at: now,
                value: st.value.clone().or_else(|| st.why.clone()),
                reason: (*reason).into(),
            };
            if let Err(e) = self.store.record_transition(&t) {
                tracing::error!(error = %e, rule = rule.name, "alert transition not recorded");
            }
        }
        if !step.transitions.is_empty()
            || !step.notices.is_empty()
            || st.phase != Phase::Ok
            || st.misses > 0
        {
            self.persist(rule.name, &object, &st);
        }
        step.notices
            .into_iter()
            .map(|kind| Notice {
                rule,
                object: object.clone(),
                kind,
                severity: st.severity.unwrap_or(rule.severity),
                value: st
                    .value
                    .clone()
                    .or_else(|| st.why.clone())
                    .unwrap_or_default(),
                threshold: st.threshold.clone().unwrap_or_default(),
                since: st.firing_since.unwrap_or(st.since),
                at: now,
                under: None,
            })
            .collect()
    }

    fn persist(&self, rule: &str, object: &str, st: &AlertState) {
        let saved = serde_json::to_string(st)
            .map_err(|e| e.to_string())
            .and_then(|json| self.store.save_state(rule, object, &json).map_err(|e| e.0));
        if let Err(e) = saved {
            tracing::error!(error = %e, rule, "alert state not saved");
        }
    }

    /// The alert a dependency stands for, while it is in play.
    fn covering(&self, dep: Dep) -> Option<&'static str> {
        let name = match dep {
            Dep::KevyServer => rules::KEVY_SERVER_UNAVAILABLE,
            Dep::ReportsArriving => rules::NO_REPORTS,
        };
        self.states
            .iter()
            .any(|((r, _), st)| r == name && st.active())
            .then_some(name)
    }

    /// How many alerts are firing at critical.
    pub fn critical_firing(&self) -> usize {
        self.states
            .values()
            .filter(|st| st.phase == Phase::Firing && st.severity == Some(Severity::Critical))
            .count()
    }

    pub fn view(&self) -> Vec<AlertView> {
        let mut out: Vec<AlertView> = self
            .states
            .iter()
            .filter_map(|((r, o), st)| {
                let rule = self.rules.iter().find(|x| x.name == r)?;
                Some(AlertView {
                    rule: rule.name,
                    object: o.clone(),
                    phase: st.phase.as_str(),
                    severity: st.severity.unwrap_or(rule.severity).as_str(),
                    since: st.since,
                    value: st.value.clone(),
                    threshold: st.threshold.clone(),
                    why: st.why.clone(),
                    title_en: rule.text.title_en,
                    title_zh: rule.text.title_zh,
                })
            })
            .collect();
        out.sort_by_key(|a| (a.phase == "ok", a.rule, a.object.clone()));
        out
    }

    pub fn state(&self, rule: &str, object: &str) -> Option<&AlertState> {
        self.states.get(&(rule.to_string(), object.to_string()))
    }
}

#[cfg(test)]
mod tests;
