//! Agent-key scopes on the MCP surface.
//!
//! Every tool belongs to one [`ToolCategory`]; [`McpIdentity::check_tool`]
//! runs before every tool call (see `call_tool` in `mod.rs`) and refuses
//! a tool the caller's scopes do not cover. The same vocabulary as REST:
//! `admin` → every tool, `mail:write` → read + write + mock, `mail:read` →
//! read + reading mock mail, `mock` → mock tools, `mock:{ns}` → mock tools on namespace `ns` only.
//! A browser session or an unscoped key is unrestricted.

use rmcp::ErrorData as McpError;
use rmcp::model::JsonObject;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolCategory {
    Read,
    Write,
    Admin,
    Mock,
}

use ToolCategory::{Admin, Read, Write};

/// Every tool whose name does not start with `mock_`. Tools named `mock_*`
/// are [`ToolCategory::Mock`] without an entry here.
const TOOL_CATEGORIES: &[(&str, ToolCategory)] = &[
    ("add_account_to_group", Admin),
    ("add_alias", Admin),
    ("add_domain", Admin),
    ("add_email_group_member", Admin),
    ("archive_thread", Write),
    ("audit_list_conversations", Admin),
    ("audit_read_thread", Admin),
    ("cancel_scheduled", Write),
    ("create_account", Admin),
    ("create_email_group", Admin),
    ("create_webhook", Admin),
    ("delete_draft", Write),
    ("delete_email_group", Admin),
    ("delete_encryption_key", Write),
    ("delete_signature", Write),
    ("delete_template", Write),
    ("delete_thread", Write),
    ("delete_webhook", Admin),
    ("get_account_permissions", Admin),
    ("get_account_send_as", Admin),
    ("get_audit_log", Admin),
    ("get_categories", Read),
    ("get_email_group_members", Admin),
    ("get_fraud_verdict", Read),
    ("get_inbox_metrics", Read),
    ("get_my_permissions", Read),
    ("get_public_key_of", Read),
    ("get_queue", Read),
    ("get_server_info", Read),
    ("get_system_config", Admin),
    ("get_thread_summary", Read),
    ("greylist_local_add", Admin),
    ("greylist_local_remove", Admin),
    ("list_accounts", Admin),
    ("list_admin_queue", Admin),
    ("list_aliases_admin", Admin),
    ("list_conversations", Read),
    ("list_domains", Admin),
    ("list_drafts", Read),
    ("list_email_groups", Admin),
    ("list_failed_outbound", Admin),
    ("list_greylist_local", Admin),
    ("list_groups", Admin),
    ("list_mailboxes", Read),
    ("list_own_encryption_keys", Read),
    ("list_own_scheduled", Read),
    ("list_quarantined", Read),
    ("list_scheduled_outbound", Admin),
    ("list_signatures", Read),
    ("list_templates", Read),
    ("list_webhooks", Admin),
    ("mark_all_read", Write),
    ("mark_thread_read", Write),
    ("mark_thread_unread", Write),
    ("pin_thread", Write),
    ("read_thread", Read),
    ("reconcile_maildir", Admin),
    ("release_quarantined", Write),
    ("remove_account", Admin),
    ("remove_account_from_group", Admin),
    ("remove_alias", Admin),
    ("remove_domain", Admin),
    ("remove_email_group_member", Admin),
    ("reply_email", Write),
    ("reschedule_scheduled", Write),
    ("reset_system_config", Admin),
    ("retry_queue_message", Admin),
    ("save_draft", Write),
    ("save_signature", Write),
    ("save_template", Write),
    ("search_contacts", Read),
    ("search_conversations", Read),
    ("send_email", Write),
    ("send_scheduled_email", Write),
    ("set_account_password", Admin),
    ("set_account_send_as", Admin),
    ("set_encryption_key", Write),
    ("set_system_config", Admin),
    ("snooze_thread", Write),
    ("star_thread", Write),
    ("unarchive_thread", Write),
    ("unpin_thread", Write),
    ("unsnooze_thread", Write),
    ("unstar_thread", Write),
];

/// Mock tools a `mock:{ns}` key may not call at all.
const NAMESPACE_MANAGEMENT_TOOLS: &[&str] = &["mock_create_namespace", "mock_delete_namespace"];

/// Mock tool a `mock:{ns}` key may call without a `namespace` argument;
/// the tool itself filters with [`McpIdentity::mock_namespace_allowed`].
const NAMESPACE_LISTING_TOOL: &str = "mock_list_namespaces";

pub(crate) fn tool_category(name: &str) -> Option<ToolCategory> {
    if name.starts_with("mock_") {
        return Some(ToolCategory::Mock);
    }
    TOOL_CATEGORIES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// Who an MCP session acts as. `scopes: None` = unrestricted (browser
/// session, or an agent key created without scopes).
#[derive(Debug, Clone, Default)]
pub struct McpIdentity {
    pub user: String,
    pub scopes: Option<Vec<String>>,
}

impl McpIdentity {
    pub fn new(user: String, scopes: Vec<String>) -> Self {
        let scopes = match scopes.is_empty() {
            true => None,
            false => Some(scopes),
        };
        Self { user, scopes }
    }

    /// The single scope gate, run before every tool call.
    pub(crate) fn check_tool(&self, name: &str, args: Option<&JsonObject>) -> Result<(), McpError> {
        let Some(scopes) = &self.scopes else {
            return Ok(());
        };
        let allowed = tool_category(name)
            .is_some_and(|category| scopes.iter().any(|s| scope_allows(s, category, name, args)));
        match allowed {
            true => Ok(()),
            false => Err(McpError::invalid_request(
                "scope does not allow this tool",
                None,
            )),
        }
    }

    /// Whether this caller may touch mock namespace `ns`. Mock tools that
    /// take a namespace other than through a `namespace` argument, or that
    /// list namespaces, must call this.
    pub fn mock_namespace_allowed(&self, ns: &str) -> bool {
        match self.mock_ns_restriction() {
            None => true,
            Some(list) => list.iter().any(|n| n == ns),
        }
    }

    /// Whether this caller may create or delete namespaces.
    pub fn can_manage_namespaces(&self) -> bool {
        self.mock_ns_restriction().is_none()
    }

    fn mock_ns_restriction(&self) -> Option<Vec<String>> {
        self.scopes
            .as_deref()
            .and_then(crate::session::mock_ns_restriction)
    }
}

fn scope_allows(
    scope: &str,
    category: ToolCategory,
    name: &str,
    args: Option<&JsonObject>,
) -> bool {
    match (scope, category) {
        ("admin", _) => true,
        ("mail:write", Read | Write) => true,
        ("mail:read", Read) => true,
        // as on REST: mail scopes cover the owner's mock mail too
        ("mock" | "mail:write", ToolCategory::Mock) => true,
        ("mail:read", ToolCategory::Mock) => MOCK_READ_TOOLS.contains(&name),
        (_, ToolCategory::Mock) => match scope.strip_prefix("mock:") {
            Some(ns) => mock_ns_tool_allowed(ns, name, args),
            None => false,
        },
        _ => false,
    }
}

const MOCK_READ_TOOLS: &[&str] = &[
    "mock_list_namespaces",
    "mock_list_messages",
    "mock_wait_for_message",
    "mock_read_message",
];

fn mock_ns_tool_allowed(ns: &str, name: &str, args: Option<&JsonObject>) -> bool {
    if NAMESPACE_MANAGEMENT_TOOLS.contains(&name) {
        return false;
    }
    let arg = args
        .and_then(|a| a.get("namespace"))
        .and_then(|v| v.as_str());
    match arg {
        Some(given) => given == ns,
        None => name == NAMESPACE_LISTING_TOOL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::mcp::MailrsMcpService;

    fn id(scopes: &[&str]) -> McpIdentity {
        McpIdentity::new(
            "u@example.com".into(),
            scopes.iter().map(|s| s.to_string()).collect(),
        )
    }

    fn ns_args(ns: &str) -> JsonObject {
        let mut m = JsonObject::new();
        m.insert("namespace".into(), ns.into());
        m
    }

    #[test]
    fn every_registered_tool_has_a_category_and_no_entry_is_stale() {
        let registered: std::collections::BTreeSet<String> = MailrsMcpService::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        assert!(registered.len() >= TOOL_CATEGORIES.len());
        for name in &registered {
            assert!(
                tool_category(name).is_some(),
                "{name} has no scope category"
            );
        }
        for (name, _) in TOOL_CATEGORIES {
            assert!(
                registered.contains(*name),
                "{name} is not a registered tool"
            );
            assert!(
                !name.starts_with("mock_"),
                "{name} is covered by the prefix rule"
            );
        }
        assert!(
            TOOL_CATEGORIES.windows(2).all(|w| w[0].0 < w[1].0),
            "table must be sorted and free of duplicates"
        );
    }

    #[test]
    fn unscoped_identity_may_call_anything() {
        let i = id(&[]);
        assert!(i.scopes.is_none());
        for name in [
            "send_email",
            "create_account",
            "mock_create_namespace",
            "nope",
        ] {
            assert!(i.check_tool(name, None).is_ok());
        }
        assert!(i.can_manage_namespaces());
    }

    #[test]
    fn category_matrix() {
        let cases: &[(&[&str], &str, bool)] = &[
            (&["mail:read"], "list_conversations", true),
            (&["mail:read"], "send_email", false),
            (&["mail:read"], "list_accounts", false),
            (&["mail:read"], "mock_list_messages", true),
            (&["mail:read"], "mock_send", false),
            (&["mail:write"], "list_conversations", true),
            (&["mail:write"], "send_email", true),
            (&["mail:write"], "create_account", false),
            (&["mail:write"], "mock_send", true),
            (&["admin"], "create_account", true),
            (&["admin"], "send_email", true),
            (&["admin"], "mock_create_namespace", true),
            (&["mock"], "mock_create_namespace", true),
            (&["mock"], "list_conversations", false),
            (&["mock"], "send_email", false),
            (&["mail:read", "mock"], "list_conversations", true),
            (&["mail:read", "mock"], "mock_list_messages", true),
            (&["bogus"], "list_conversations", false),
        ];
        for (scopes, tool, want) in cases {
            let got = id(scopes).check_tool(tool, None).is_ok();
            assert_eq!(got, *want, "{scopes:?} calling {tool}");
        }
    }

    #[test]
    fn mail_read_identity_is_refused_a_write_tool() {
        let err = id(&["mail:read"])
            .check_tool("delete_thread", None)
            .unwrap_err();
        assert_eq!(err.message, "scope does not allow this tool");
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_REQUEST);
    }

    #[test]
    fn scoped_identity_is_refused_an_unlisted_tool() {
        assert!(id(&["mail:write"]).check_tool("not_a_tool", None).is_err());
        assert!(id(&["admin"]).check_tool("not_a_tool", None).is_err());
    }

    #[test]
    fn mock_ns_scope_is_held_to_its_namespace() {
        let i = id(&["mock:acme"]);
        let own = ns_args("acme");
        let other = ns_args("other");
        assert!(i.check_tool("mock_list_messages", Some(&own)).is_ok());
        assert!(i.check_tool("mock_list_messages", Some(&other)).is_err());
        assert!(i.check_tool("mock_list_messages", None).is_err());
        assert!(i.check_tool("mock_list_namespaces", None).is_ok());
        assert!(i.check_tool("mock_create_namespace", Some(&own)).is_err());
        assert!(i.check_tool("mock_delete_namespace", Some(&own)).is_err());
        assert!(i.check_tool("list_conversations", None).is_err());
        assert!(i.mock_namespace_allowed("acme"));
        assert!(!i.mock_namespace_allowed("other"));
        assert!(!i.can_manage_namespaces());
    }

    #[test]
    fn mock_scope_reaches_every_namespace() {
        let i = id(&["mock"]);
        assert!(
            i.check_tool("mock_list_messages", Some(&ns_args("x")))
                .is_ok()
        );
        assert!(i.mock_namespace_allowed("anything"));
        assert!(i.can_manage_namespaces());
    }
}
