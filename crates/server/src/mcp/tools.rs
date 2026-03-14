use serde::Deserialize;

// --- parameter structs ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct Attachment {
    /// filename (e.g. "photo.png")
    pub filename: String,
    /// MIME type (e.g. "image/png")
    pub content_type: String,
    /// base64-encoded file content
    pub data: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SendEmailParams {
    /// sender email address (omit to use authenticated account)
    #[serde(default)]
    pub from: Option<String>,
    /// recipient email addresses
    pub to: Vec<String>,
    /// CC recipients
    #[serde(default)]
    pub cc: Option<Vec<String>>,
    /// email subject
    pub subject: String,
    /// plain text email body
    pub body: String,
    /// optional HTML email body
    #[serde(default)]
    pub html_body: Option<String>,
    /// optional file attachments (base64-encoded)
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReadEmailParams {
    /// message UID from list_conversations or search_emails results
    pub uid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SearchEmailsParams {
    /// search query string
    pub query: String,
    /// max results (default 20, max 20)
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReplyEmailParams {
    /// thread ID to reply to (from list_conversations)
    pub thread_id: String,
    /// reply text body
    pub body: String,
    /// sender email address (omit to use authenticated account)
    #[serde(default)]
    pub from: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListConversationsParams {
    /// max results (default 20, max 20)
    #[serde(default)]
    pub limit: Option<u32>,
    /// filter by category: personal, notification, promotion, general
    #[serde(default)]
    pub category: Option<String>,
}

// --- admin / user management parameter structs ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateAccountParams {
    /// email address (e.g. "user@golia.jp")
    pub address: String,
    /// domain name (e.g. "golia.jp")
    pub domain: String,
    /// display name
    #[serde(default)]
    pub display_name: String,
    /// password (will be argon2-hashed)
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAccountParams {
    /// email address to remove
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListAccountsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct SetAccountPasswordParams {
    /// email address
    pub address: String,
    /// new password (will be argon2-hashed)
    pub password: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ListGroupsParams {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAccountGroupsParams {
    /// email address
    pub address: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct AddAccountToGroupParams {
    /// email address
    pub address: String,
    /// group ID
    pub group_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct RemoveAccountFromGroupParams {
    /// email address
    pub address: String,
    /// group ID
    pub group_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct GetAccountPermissionsParams {
    /// email address
    pub address: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;

    #[test]
    fn send_email_params_schema_generation() {
        let schema = schema_for!(SendEmailParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("to"));
        assert!(json.contains("subject"));
        assert!(json.contains("body"));
    }

    #[test]
    fn read_email_params_schema_generation() {
        let schema = schema_for!(ReadEmailParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("uid"));
    }

    #[test]
    fn search_emails_params_schema_generation() {
        let schema = schema_for!(SearchEmailsParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("query"));
        assert!(json.contains("limit"));
    }

    #[test]
    fn reply_email_params_schema_generation() {
        let schema = schema_for!(ReplyEmailParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("thread_id"));
        assert!(json.contains("body"));
    }

    #[test]
    fn list_conversations_params_schema_generation() {
        let schema = schema_for!(ListConversationsParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("limit"));
        assert!(json.contains("category"));
    }

    #[test]
    fn send_email_params_deserialize_empty_to() {
        let json = r#"{"to": [], "subject": "test", "body": "hello"}"#;
        let params: SendEmailParams = serde_json::from_str(json).unwrap();
        assert!(params.to.is_empty());
    }

    #[test]
    fn send_email_params_deserialize_with_optional_fields() {
        let json = r#"{"to": ["a@b.com"], "subject": "test", "body": "hello", "from": "x@y.com", "cc": ["c@d.com"]}"#;
        let params: SendEmailParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.from.as_deref(), Some("x@y.com"));
        assert_eq!(params.cc.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn list_conversations_params_defaults() {
        let json = r#"{}"#;
        let params: ListConversationsParams = serde_json::from_str(json).unwrap();
        assert!(params.limit.is_none());
        assert!(params.category.is_none());
    }

    #[test]
    fn create_account_params_schema_generation() {
        let schema = schema_for!(CreateAccountParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("address"));
        assert!(json.contains("domain"));
    }

    #[test]
    fn create_account_params_deserialize() {
        let json = r#"{"address": "new@golia.jp", "domain": "golia.jp", "display_name": "New", "password": "secret"}"#;
        let params: CreateAccountParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.address, "new@golia.jp");
        assert_eq!(params.domain, "golia.jp");
    }

    #[test]
    fn create_account_params_defaults() {
        let json = r#"{"address": "a@b.com", "domain": "b.com"}"#;
        let params: CreateAccountParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.display_name, "");
        assert_eq!(params.password, "");
    }

    #[test]
    fn add_account_to_group_params_schema() {
        let schema = schema_for!(AddAccountToGroupParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("address"));
        assert!(json.contains("group_id"));
    }

    #[test]
    fn get_account_permissions_params_schema() {
        let schema = schema_for!(GetAccountPermissionsParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("address"));
    }

    #[test]
    fn list_accounts_params_empty() {
        let json = r#"{}"#;
        let _params: ListAccountsParams = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn list_groups_params_empty() {
        let json = r#"{}"#;
        let _params: ListGroupsParams = serde_json::from_str(json).unwrap();
    }
}
