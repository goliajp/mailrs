//! Params for the rule tools (batch 17).

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateRuleParams {
    /// 1 to 64 characters, unique for this user ignoring case.
    pub name: String,
    /// A search query, as search_conversations takes it, without `in:`,
    /// `is:` or `label:`: those say where mail is or what was done to it,
    /// which a message does not have as it arrives.
    pub query: String,
    /// What to do with matching mail, in order: `label:<id>`, `archive`,
    /// `read`, `star`, `junk`, `trash`.
    pub actions: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RuleIdParams {
    /// The rule's id, from list_rules.
    pub rule_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PreviewRuleParams {
    /// The query a rule would have.
    pub query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunIdParams {
    /// The run's id, from apply_rule.
    pub run_id: String,
}
